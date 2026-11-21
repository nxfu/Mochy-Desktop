//! PO tokens — the port of `utils/potoken/` (5 files).
//!
//! The two tokens produced by one BotGuard minting cycle, and the
//! pairing that is **not intuitive** (PoTokenResult.kt): the
//! **session**-bound token belongs in the player request
//! (`serviceIntegrityDimensions`), and the **video**-bound token is
//! the one googlevideo wants on the stream url as `pot=`. The field
//! names describe where each token goes rather than what it was
//! minted from — upstream had this backwards once, and the mistake is
//! called out so it cannot return.
//!
//! The mint itself (PoTokenWebView.kt + assets/po_token.html) runs
//! **in-process**: the challenge comes from the transport's BotGuard
//! `Create` endpoint, the VM script and program execute in the
//! shared QuickJS engine (the adapted
//! `assets/potoken/botguard_client.js`), the response goes to
//! `GenerateIT`, and the minter mints per identifier. Upstream needed
//! a WebView for this; a desktop process does not.
//!
//! `rustypipe-botguard` — the obvious off-the-shelf candidate — is a
//! **Deno-based CLI tool** on crates.io (JSDOM browser emulation,
//! snapshot files), not an in-process library, so the port implements
//! the same flow (Create/GenerateIT + LuanRT's BgUtils client shapes)
//! over its own engine. A deno_core-based alternative — embedding a
//! stripped Deno runtime the way rustypipe-botguard does — stays
//! documented here and UNUSED; see HANDOFF Phase 06.
//!
//! Failure discipline (PoTokenGenerator.kt:25-86): a failed mint
//! starts a 5-minute cooldown so a broken BotGuard never freezes the
//! player, and a mint that fails on a warm engine retries once with a
//! fresh one.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::Value as Json;

use super::jsengine::{Engine, JsError};
use crate::error::InnertubeError;
use crate::transport::InnerTubeTransport;

/// The adapted BotGuard client
/// (`assets/potoken/botguard_client.js`, from upstream's
/// `assets/po_token.html`).
const BOTGUARD_CLIENT_JS: &str = include_str!("../../assets/potoken/botguard_client.js");

/// `GOOGLE_API_KEY` (PoTokenWebView.kt:303) — the public web key the
/// BotGuard service answers to; the transport's Create/GenerateIT
/// endpoints carry it.
pub const BOTGUARD_REQUEST_KEY: &str = "O43z0dpjhgX20SCx4KAo";

/// The cold-start bound (PoTokenWebView.kt:310) — the engine boot
/// plus the BotGuard round trips.
const COLD_START_TIMEOUT: Duration = Duration::from_secs(45);

/// The warm-path bound (PoTokenWebView.kt:313) — minting one token
/// over a live engine.
const MINT_TIMEOUT: Duration = Duration::from_secs(10);

/// A failed mint stops trying for this long
/// (PoTokenGenerator.kt:24-25).
const MINT_COOLDOWN: Duration = Duration::from_secs(5 * 60);

/// The minter is rebuilt ten minutes before the integrity token
/// expires (PoTokenWebView.kt:139).
const EXPIRY_MARGIN: Duration = Duration::from_secs(10 * 60);

/// Any well-known video id works to boot the engine; the resulting
/// player token is discarded and only the session token it mints
/// alongside is kept (PoTokenGenerator.kt:21).
const PREWARM_VIDEO_ID: &str = "dQw4w9WgXcQ";

/// A PO-token failure.
#[derive(Debug, thiserror::Error)]
pub enum PoTokenError {
    #[error("botguard: {0}")]
    BotGuard(String),
    #[error("transport: {0}")]
    Transport(#[from] InnertubeError),
    #[error("js: {0}")]
    Js(#[from] JsError),
}

/// `PoTokenResult` (PoTokenResult.kt) — the deliberate swap:
/// the session token rides the player request, the per-video token
/// rides the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoTokenResult {
    /// Minted from the session id. Sent in the player request's
    /// `serviceIntegrityDimensions`.
    pub player_request_po_token: String,
    /// Minted from the video id. Appended to the stream url as
    /// `pot=`.
    pub streaming_data_po_token: String,
}

/// The BotGuard engine session: challenge solved, integrity token in
/// hand, minter live in the QuickJS engine.
pub struct BotGuardSession {
    engine: Arc<Engine>,
    /// When the integrity token expires (already minus the ten-minute
    /// margin).
    expires_at: Instant,
}

impl BotGuardSession {
    /// Boot the engine: Create → run the challenge → GenerateIT →
    /// create the minter (PoTokenWebView.kt:94-171, the
    /// `downloadAndRunBotguard`/`onRunBotguardResult`/
    /// `onMinterCreated` chain over the transport's endpoints).
    pub async fn boot(
        engine: Arc<Engine>,
        transport: &InnerTubeTransport,
    ) -> Result<Self, PoTokenError> {
        engine.eval_sloppy::<()>(BOTGUARD_CLIENT_JS).await?;

        // 1. Create — the challenge bytes
        let raw_challenge = transport.botguard_create(BOTGUARD_REQUEST_KEY).await?;
        let challenge = parse_challenge_data(&raw_challenge)?;

        // 2. run the VM script + program, take the snapshot
        let challenge_json = serde_json::to_string(&challenge)
            .map_err(|error| PoTokenError::BotGuard(error.to_string()))?;
        let start = format!(
            "(function () {{ return globalThis.__bgRun({}); }})()",
            serde_json::to_string(&challenge_json).unwrap_or_default()
        );
        engine.eval_sloppy::<bool>(start).await?;
        let settled = settle(&engine, COLD_START_TIMEOUT).await?;
        let botguard_response = settled_ok(settled)
            .ok_or_else(|| PoTokenError::BotGuard("runBotGuard failed".into()))?
            .to_string();

        // 3. GenerateIT — the integrity token
        let raw_integrity = transport
            .botguard_generate_it(BOTGUARD_REQUEST_KEY, &botguard_response)
            .await?;
        let (integrity_bytes, expires_in_seconds) = parse_integrity_token_data(&raw_integrity)?;

        // 4. create the minter
        let bytes_json = serde_json::to_string(&integrity_bytes).unwrap_or_default();
        let minter_start = format!(
            "(function () {{ return globalThis.__bgCreateMinter({}); }})()",
            serde_json::to_string(&bytes_json).unwrap_or_default()
        );
        engine.eval_sloppy::<bool>(minter_start).await?;
        let settled = settle(&engine, COLD_START_TIMEOUT).await?;
        if settled_ok(settled).is_none() {
            return Err(PoTokenError::BotGuard("createPoTokenMinter failed".into()));
        }

        Ok(BotGuardSession {
            engine,
            expires_at: Instant::now() + Duration::from_secs(expires_in_seconds.max(1))
                - EXPIRY_MARGIN,
        })
    }

    /// `isExpired` (PoTokenWebView.kt:234-235).
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }

    /// `generatePoToken` (PoTokenWebView.kt:173-210): mint one token
    /// for `identifier` — the session id or the video id — and return
    /// its URL-safe base64 form (`u8ToBase64`,
    /// JavaScriptUtil.kt:70-78).
    pub async fn mint(&self, identifier: &str) -> Result<String, PoTokenError> {
        let bytes: Vec<u8> = identifier.as_bytes().to_vec();
        let bytes_json = serde_json::to_string(&bytes).unwrap_or_default();
        let start = format!(
            "(function () {{ return globalThis.__bgMint({}); }})()",
            serde_json::to_string(&bytes_json).unwrap_or_default()
        );
        self.engine.eval_sloppy::<bool>(start).await?;
        let settled = settle(&self.engine, MINT_TIMEOUT).await?;
        let value = settled_ok(settled)
            .ok_or_else(|| PoTokenError::BotGuard("obtainPoToken failed".into()))?;
        let token_bytes: Vec<u8> = serde_json::from_value(value)
            .map_err(|error| PoTokenError::BotGuard(error.to_string()))?;
        // JavaScriptUtil.kt:70-78 `u8ToBase64`: okio's base64() emits
        // PADDED base64, then only the alphabet is swapped — the '='
        // padding stays (the 2026-11-21 audit fixed the port from
        // stripping it)
        Ok(base64::engine::general_purpose::URL_SAFE.encode(token_bytes))
    }
}

/// The settled glue value: `Some(value)` on `{ok: true}`.
fn settled_ok(settled: Json) -> Option<Json> {
    (settled.get("ok").and_then(Json::as_bool) == Some(true))
        .then(|| settled.get("value").cloned().unwrap_or(Json::Null))
}

/// Wait for the glue's `__bgDone` completion while draining the job
/// queue and the cooperative timers — the engine-side half of the
/// JavascriptInterface bridge.
async fn settle(engine: &Arc<Engine>, timeout: Duration) -> Result<Json, PoTokenError> {
    let raw = engine
        .drive(move |runtime, context| {
            Engine::wait_for(
                runtime,
                context,
                |ctx| {
                    let done: Option<String> = ctx
                        .eval("globalThis.__bgDone ? JSON.stringify(globalThis.__bgDone) : null")
                        .map_err(|error| JsError::Script(error.to_string()))?;
                    done.map(Ok).transpose()
                },
                timeout,
            )
        })
        .await?;
    serde_json::from_str(&raw).map_err(|error| PoTokenError::BotGuard(error.to_string()))
}

/// One minted session held by the generator
/// (`getWebClientPoToken`'s locked state, PoTokenGenerator.kt:112-169).
struct Session {
    botguard: Arc<BotGuardSession>,
    session_id: String,
    /// The session-bound token (`webPoTokenStreamingPot`).
    streaming_pot: String,
}

/// The PO-token generator — the `PoTokenGenerator` port
/// (PoTokenGenerator.kt:14-170): one cached session per session id,
/// recreated on expiry or session change, a 5-minute cooldown after a
/// failed mint, and one forced-recreate retry when a warm mint fails.
pub struct PoTokenGenerator {
    engine: Arc<Engine>,
    state: tokio::sync::Mutex<Option<Session>>,
    mint_cooldown_until: tokio::sync::Mutex<Option<Instant>>,
}

impl PoTokenGenerator {
    pub fn new(engine: Arc<Engine>) -> Self {
        PoTokenGenerator {
            engine,
            state: tokio::sync::Mutex::const_new(None),
            mint_cooldown_until: tokio::sync::Mutex::const_new(None),
        }
    }

    /// True once a session token has been minted and the engine is not
    /// expired (`hasSessionToken`, PoTokenGenerator.kt:43-44) —
    /// surfaced in diagnostics.
    pub fn has_session_token(&self) -> bool {
        match self.state.try_lock() {
            Ok(guard) => guard
                .as_ref()
                .map(|session| !session.botguard.is_expired())
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Boot the BotGuard engine before the first track needs it
    /// (`preWarm`, PoTokenGenerator.kt:53-59): a cold start costs tens
    /// of seconds, and paying that inside the resolver stalls the
    /// first song.
    pub async fn prewarm(&self, transport: &InnerTubeTransport, session_id: &str) {
        if self
            .po_token_for(transport, PREWARM_VIDEO_ID, session_id)
            .await
            .is_some()
        {
            tracing::debug!(target: "mochy_potoken", "po token pre-warm complete");
        } else {
            tracing::warn!(target: "mochy_potoken", "po token pre-warm did not produce a token");
        }
    }

    /// `poTokenFor` (PoTokenGenerator.kt:65-82): the suspend entry
    /// point the walk calls. The cooldown and every failure discipline
    /// live here — a `None` answer is always acceptable to the caller
    /// (the walk falls back to a client that needs no token).
    pub async fn po_token_for(
        &self,
        transport: &InnerTubeTransport,
        video_id: &str,
        session_id: &str,
    ) -> Option<PoTokenResult> {
        if let Some(until) = *self.mint_cooldown_until.lock().await
            && Instant::now() < until
        {
            tracing::debug!(target: "mochy_potoken", "skipping po token mint - cooling down after a failure");
            return None;
        }
        match self
            .web_client_po_token(transport, video_id, session_id, false)
            .await
        {
            Ok(result) => Some(result),
            Err(error) => {
                tracing::error!(target: "mochy_potoken", error = %error, "po token generation failed");
                *self.mint_cooldown_until.lock().await = Some(Instant::now() + MINT_COOLDOWN);
                None
            }
        }
    }

    /// `getWebClientPoToken` (PoTokenGenerator.kt:112-169): recreate
    /// the session when it must be, mint the video token, retry once
    /// with a forced recreate when a warm mint fails.
    async fn web_client_po_token(
        &self,
        transport: &InnerTubeTransport,
        video_id: &str,
        session_id: &str,
        force_recreate: bool,
    ) -> Result<PoTokenResult, PoTokenError> {
        let mut guard = self.state.lock().await;
        let should_recreate = force_recreate
            || guard
                .as_ref()
                .map(|session| session.botguard.is_expired() || session.session_id != session_id)
                .unwrap_or(true);

        let has_been_recreated = should_recreate;
        if should_recreate {
            // upstream clears the session slot BEFORE recreating
            // (PoTokenGenerator.kt:124-133): a failed boot or session
            // mint must leave no stale session behind — the next call
            // starts over instead of serving the old token
            guard.take();
            let botguard =
                Arc::new(BotGuardSession::boot(Arc::clone(&self.engine), transport).await?);
            let streaming_pot = botguard.mint(session_id).await?;
            *guard = Some(Session {
                botguard,
                session_id: session_id.to_owned(),
                streaming_pot,
            });
        }
        let session = guard.as_ref().expect("session just ensured");

        // the video-bound token; a warm-engine failure retries once
        // with a fresh engine (PoTokenGenerator.kt:149-160)
        let player_pot = match session.botguard.mint(video_id).await {
            Ok(token) => token,
            Err(error) if !has_been_recreated => {
                tracing::error!(target: "mochy_potoken", error = %error, "failed to obtain the po token, retrying");
                drop(guard);
                return Box::pin(self.web_client_po_token(transport, video_id, session_id, true))
                    .await;
            }
            Err(error) => return Err(error),
        };

        // Session token in the player request, video token on the
        // stream url — see PoTokenResult.
        Ok(PoTokenResult {
            player_request_po_token: session.streaming_pot.clone(),
            streaming_data_po_token: player_pot,
        })
    }
}

// --- JavaScriptUtil.kt ------------------------------------------------

/// `parseChallengeData` (JavaScriptUtil.kt:15-59): unwrap the
/// scrambled-or-plain challenge array into the object the BotGuard
/// client expects.
pub(crate) fn parse_challenge_data(raw: &str) -> Result<Json, PoTokenError> {
    let bot = |message: &str| PoTokenError::BotGuard(message.to_owned());
    let scrambled: Vec<Json> =
        serde_json::from_str(raw).map_err(|error| bot(&format!("challenge parse: {error}")))?;
    let challenge: Vec<Json> =
        if scrambled.len() > 1 && scrambled[1].as_str().is_some_and(|value| !value.is_empty()) {
            let descrambled = descramble(scrambled[1].as_str().expect("checked"))?;
            serde_json::from_str(&descrambled)
                .map_err(|error| bot(&format!("descramble parse: {error}")))?
        } else {
            scrambled[0]
                .as_array()
                .cloned()
                .ok_or_else(|| bot("challenge element 0 is not an array"))?
        };

    let field = |index: usize| {
        challenge
            .get(index)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or_else(|| bot(&format!("challenge field {index} missing")))
    };
    let wrapped_value = |index: usize| -> Result<Json, PoTokenError> {
        match challenge.get(index) {
            Some(Json::Null) | None => Ok(Json::Null),
            Some(array) => {
                let found = array
                    .as_array()
                    .and_then(|items| items.iter().find(|item| item.as_str().is_some()).cloned())
                    .unwrap_or(Json::Null);
                Ok(found)
            }
        }
    };

    Ok(serde_json::json!({
        "messageId": field(0)?,
        "interpreterJavascript": {
            "privateDoNotAccessOrElseSafeScriptWrappedValue": wrapped_value(1)?,
            "privateDoNotAccessOrElseTrustedResourceUrlWrappedValue": wrapped_value(2)?,
        },
        "interpreterHash": field(3)?,
        "program": field(4)?,
        "globalName": field(5)?,
        "clientExperimentsStateBlob": field(7)?,
    }))
}

/// `descramble` (JavaScriptUtil.kt:80-85): base64url → each byte +97
/// → the inner JSON.
fn descramble(scrambled: &str) -> Result<String, PoTokenError> {
    let bytes = base64url_decode(scrambled)?;
    let shifted: Vec<u8> = bytes.iter().map(|byte| byte.wrapping_add(97)).collect();
    String::from_utf8(shifted)
        .map_err(|error| PoTokenError::BotGuard(format!("descramble utf-8: {error}")))
}

/// `parseIntegrityTokenData` (JavaScriptUtil.kt:61-63): the token
/// bytes and the expiry in seconds.
pub(crate) fn parse_integrity_token_data(raw: &str) -> Result<(Vec<u8>, u64), PoTokenError> {
    let bot = |message: &str| PoTokenError::BotGuard(message.to_owned());
    let array: Vec<Json> =
        serde_json::from_str(raw).map_err(|error| bot(&format!("integrity parse: {error}")))?;
    let token_b64 = array
        .first()
        .and_then(|value| value.as_str())
        .ok_or_else(|| bot("integrity token missing"))?;
    let expires = array
        .get(1)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| bot("integrity expiry missing"))?;
    Ok((base64url_decode(token_b64)?, expires))
}

/// `base64ToByteString` (JavaScriptUtil.kt:95-103): the BotGuard
/// base64 dialect — `-`→`+`, `_`→`/`, `.`→`=` — then standard base64,
/// tolerating a missing pad.
fn base64url_decode(value: &str) -> Result<Vec<u8>, PoTokenError> {
    let normalized: String = value
        .chars()
        .map(|character| match character {
            '-' => '+',
            '_' => '/',
            '.' => '=',
            other => other,
        })
        .collect();
    let padded = match normalized.len() % 4 {
        2 => format!("{normalized}=="),
        3 => format!("{normalized}="),
        _ => normalized,
    };
    base64::engine::general_purpose::STANDARD
        .decode(padded.as_bytes())
        .map_err(|error| PoTokenError::BotGuard(format!("base64 decode: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_challenge() {
        let raw = r#"[["m-id",["script"],null,"hash","program","globalName",null,"blob"]]"#;
        let challenge = parse_challenge_data(raw).unwrap();
        assert_eq!(challenge["messageId"], "m-id");
        assert_eq!(
            challenge["interpreterJavascript"]["privateDoNotAccessOrElseSafeScriptWrappedValue"],
            "script"
        );
        assert_eq!(challenge["interpreterHash"], "hash");
        assert_eq!(challenge["program"], "program");
        assert_eq!(challenge["globalName"], "globalName");
        assert_eq!(challenge["clientExperimentsStateBlob"], "blob");
    }

    #[test]
    fn parses_a_scrambled_challenge() {
        // the inner array, shifted by -97 then base64url'd
        let inner = r#"["m-id",["script"],null,"hash","program","global",null,"blob"]"#;
        let shifted: Vec<u8> = inner.bytes().map(|byte| byte.wrapping_sub(97)).collect();
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(shifted);
        let raw = format!(r#"[null,"{encoded}"]"#);
        let challenge = parse_challenge_data(&raw).unwrap();
        assert_eq!(challenge["messageId"], "m-id");
        assert_eq!(challenge["globalName"], "global");
    }

    #[test]
    fn parses_the_integrity_token() {
        let bytes = vec![1u8, 2, 3, 250];
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes);
        let raw = format!(r#"["{encoded}", 3600]"#);
        let (decoded, expires) = parse_integrity_token_data(&raw).unwrap();
        assert_eq!(decoded, bytes);
        assert_eq!(expires, 3600);
    }

    #[test]
    fn base64url_decode_handles_the_botguard_dialect() {
        // standard bytes with the three swapped characters
        let bytes: Vec<u8> = (0..48u8).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let dialect: String = encoded
            .chars()
            .map(|c| match c {
                '+' => '-',
                '/' => '_',
                '=' => '.',
                other => other,
            })
            .collect();
        assert_eq!(base64url_decode(&dialect).unwrap(), bytes);
    }

    #[tokio::test]
    async fn the_glue_loads_into_the_engine() {
        // the adapted client must parse and register its glue entries
        let engine = Arc::new(Engine::new().unwrap());
        engine.eval_sloppy::<()>(BOTGUARD_CLIENT_JS).await.unwrap();
        let ready: bool = engine
            .eval(
                "typeof globalThis.__bgRun === 'function' && \
                 typeof globalThis.__bgCreateMinter === 'function' && \
                 typeof globalThis.__bgMint === 'function'",
            )
            .await
            .unwrap();
        assert!(ready);
        // a challenge without interpreter javascript settles into an
        // error, not a hang and not a synchronous throw
        engine
            .eval_sloppy::<bool>("globalThis.__bgRun('{\"messageId\":\"x\"}')")
            .await
            .unwrap();
        let settled = settle(&engine, Duration::from_secs(5)).await.unwrap();
        assert_eq!(
            settled["ok"], false,
            "expected the glue to report the failure"
        );
    }
}
