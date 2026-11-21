//! The resolution walk — `resolvePlaybackData`
//! (YTPlayerUtils.kt:349-806), the hardest single function of the
//! stream layer.
//!
//! The order, in upstream's own words: VISIONOS first — it is the
//! only client measured to serve a complete track without a PoToken;
//! the PoToken-capable web clients follow for anything it cannot
//! play, then the remaining clients for restricted content. IOS /
//! IPADOS / ANDROID_VR are last: they return clean urls that look
//! healthy and then stop dead at the ~786 KB untokened window. When
//! signed in and the main client could not serve, the account's
//! clients come first — they are what plays age-restricted and
//! premium-only songs.
//!
//! The InnerTubeX alternate engine goes before all of it when its
//! backend is selected (behind the `innertubex` feature);
//! SABR re-implementation is explicitly out of scope (decision Q7).

use std::time::Instant;

use crate::client::{ClientKey, YouTubeClient};
use crate::models::response::player::{PlayerResponse, VideoDetails};
use crate::youtube::YouTube;

use super::newpipe::{self, SignatureTimestampResult};
use super::playback::{BOT_CHECK_REST, PlaybackData, PlaybackRequest, StreamEngine, StreamError};
use super::ranking::select_audio_format_candidates;
use super::validate::{ProbeOutcome, stream_headers, validate_stream_url};

/// `RESTRICTED_STATUSES` (YTPlayerUtils.kt:158-163): playability
/// statuses that mean "this client may not serve the stream, but
/// another one can". `LOGIN_REQUIRED` belongs here — it is what
/// YouTube returns for "Sign in to confirm you're not a bot" — and
/// leaving it out meant those tracks never reached the fallback
/// clients that can play them.
const RESTRICTED_STATUSES: &[&str] = &[
    "AGE_CHECK_REQUIRED",
    "AGE_VERIFICATION_REQUIRED",
    "LOGIN_REQUIRED",
    "CONTENT_CHECK_REQUIRED",
];

/// `isBotCheck` (YTPlayerUtils.kt:142-143).
pub(crate) fn is_bot_check(response: &PlayerResponse) -> bool {
    response
        .playability_status
        .as_ref()
        .map(|status| {
            status.status == "LOGIN_REQUIRED"
                && status
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.to_lowercase().contains("bot"))
        })
        .unwrap_or(false)
}

/// `isPremiumOnly` (YTPlayerUtils.kt:149-150) — "this video is only
/// available to music premium members", the same words newpipe looks
/// for.
pub(crate) fn is_premium_only(response: &PlayerResponse) -> bool {
    response
        .playability_status
        .as_ref()
        .map(|status| {
            status.status != "OK"
                && status
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.to_lowercase().contains("premium"))
        })
        .unwrap_or(false)
}

/// `solvesN` (YTPlayerUtils.kt:145-146): the clients whose links
/// carry an `n` the player js has to solve — without it googlevideo
/// throttles or refuses them.
fn solves_n(client: &YouTubeClient) -> bool {
    client.use_web_po_tokens || super::is_account_client(client)
}

/// The main client's answer is an `Err` when the request failed —
/// `askMainClient` (YTPlayerUtils.kt:403-410) throws upstream, which
/// fails the whole walk: a network-level failure on the main client
/// is not retried against the other clients.
fn main_client_failure(error: &crate::error::InnertubeError) -> StreamError {
    if matches!(
        error,
        crate::error::InnertubeError::Net(mochy_net::NetError::YouTubeRateLimited)
    ) {
        return StreamError::RateLimited;
    }
    StreamError::Other(format!("YouTube refused the request ({error})"))
}

/// `resolvePlaybackData` (YTPlayerUtils.kt:349-806).
#[allow(clippy::too_many_lines)]
pub(crate) async fn resolve_playback_data(
    youtube: &YouTube,
    engine: &StreamEngine,
    request: &PlaybackRequest,
) -> Result<PlaybackData, StreamError> {
    let transport = youtube.transport();
    let registry = youtube.registry();
    let (is_logged_in, session_id) = {
        let session_guard = transport
            .session()
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let logged_in = session_guard.cookie.is_some();
        let id = if logged_in {
            session_guard.data_sync_id.clone()
        } else {
            session_guard.visitor_data.clone()
        };
        (logged_in, id)
    };

    // the web clients need the web player's visitor id, and the app
    // only fetches one in the background at startup
    // (YTPlayerUtils.kt:365-370)
    let missing_visitor_data = !is_logged_in
        && transport
            .session()
            .read()
            .map(|guard| guard.visitor_data.as_deref().is_none_or(str::is_empty))
            .unwrap_or(true);
    if missing_visitor_data && let Err(error) = youtube.refresh_visitor_data().await {
        tracing::warn!(target: "mochy_stream", error = %error, "could not fetch visitorData");
    }

    // innertubex goes first; whatever it can't find goes on to the
    // walk below exactly as before (YTPlayerUtils.kt:372-375)
    if engine.backend() == super::playback::StreamBackend::InnertubeX
        && let Some(found) = super::innertube_x_first(youtube, engine, request, is_logged_in).await
    {
        return Ok(found);
    }

    // NewPipe's signature-timestamp lookup is a blocking network round
    // trip, and it is only meaningful for clients that hand back
    // ciphered urls. VISIONOS does not, so resolving this eagerly made
    // every play wait for a request it had no use for — resolved
    // lazily, an ordinary play never touches it
    // (YTPlayerUtils.kt:377-381).
    let signature_timestamp = tokio::sync::OnceCell::new();
    let sts = || {
        let cell = &signature_timestamp;
        async move {
            cell.get_or_init(|| async {
                newpipe::get_signature_timestamp(&engine.cipher, transport, &request.video_id)
                    .await
                    .unwrap_or(SignatureTimestampResult {
                        timestamp: None,
                        is_age_restricted: false,
                    })
            })
            .await
        }
    };

    // why each client couldn't play, in the order tried: what the
    // error says when none can
    let mut failures: Vec<String> = Vec::new();
    // false to the end means youtube refused the upload itself, not
    // just how we asked for it
    let mut any_client_served = false;

    let mut po_token: Option<super::potoken::PoTokenResult> = None;
    let main_client = registry.get(super::walk_keys::MAIN_CLIENT_KEY);
    if main_client.use_web_po_tokens
        && let Some(session_id) = &session_id
    {
        po_token = engine
            .potoken
            .po_token_for(transport, &request.video_id, session_id)
            .await;
    }

    // null while visionos rests after a bot check: the other clients
    // are asked straight away (YTPlayerUtils.kt:413-433)
    let resting_until = *engine.visionos_resting_until.lock().await;
    let mut main_player_response: Option<PlayerResponse> = None;
    if resting_until.is_some_and(|until| Instant::now() < until) {
        tracing::debug!(target: "mochy_stream", "{} is resting after a bot check, going straight to the other clients", main_client.client_name);
    } else {
        let main_sig_timestamp = if main_client.use_signature_timestamp {
            sts().await.timestamp
        } else {
            None
        };
        let mut answer = ask_main(
            youtube,
            request,
            &main_client,
            po_token
                .as_ref()
                .map(|token| token.player_request_po_token.as_str()),
            main_sig_timestamp,
        )
        .await?;
        // the bot check burns the visitor id, not the client: a fresh
        // one usually clears it, and when it doesn't the network is
        // flagged for a while
        if is_bot_check(&answer) {
            if let Err(error) = youtube.refresh_visitor_data_for(&main_client).await {
                tracing::warn!(target: "mochy_stream", error = %error, "could not mint a fresh visitor id");
            }
            answer = ask_main(
                youtube,
                request,
                &main_client,
                po_token
                    .as_ref()
                    .map(|token| token.player_request_po_token.as_str()),
                main_sig_timestamp,
            )
            .await?;
            if is_bot_check(&answer) {
                *engine.visionos_resting_until.lock().await = Some(Instant::now() + BOT_CHECK_REST);
                tracing::warn!(target: "mochy_stream",
                    client = %main_client.client_name,
                    "bot check again; leaving the main client alone for {} minutes",
                    BOT_CHECK_REST.as_secs() / 60);
            }
        }
        main_player_response = Some(answer);
    }
    // a bot check is about the network, not the song, so it isn't
    // treated as an age gate below: that would drop the signature
    // timestamp the account's ciphered links need
    let main_bot_checked = main_player_response.as_ref().is_some_and(is_bot_check);
    // the network is flagged: when nothing else serves either, that's
    // the reason to give
    let bot_checked = main_player_response.is_none() || main_bot_checked;
    let mut premium_only = main_player_response.as_ref().is_some_and(is_premium_only);

    // the metadata client, signed in: only the signed-in lookup's
    // links count a play on the account
    // (YTPlayerUtils.kt:441-464)
    let mut metadata_response: Option<PlayerResponse> = None;
    if is_logged_in {
        let metadata_client = registry.get(super::walk_keys::METADATA_CLIENT_KEY);
        let mut meta_po_token = None;
        if metadata_client.use_web_po_tokens
            && let Some(data_sync_id) = transport
                .session()
                .read()
                .ok()
                .and_then(|guard| guard.data_sync_id.clone())
        {
            meta_po_token = engine
                .potoken
                .po_token_for(transport, &request.video_id, &data_sync_id)
                .await;
        }
        let meta_sts = if metadata_client.use_signature_timestamp {
            sts().await.timestamp
        } else {
            None
        };
        metadata_response = youtube
            .player(
                &request.video_id,
                request.playlist_id.as_deref(),
                &metadata_client,
                meta_sts,
                meta_po_token
                    .as_ref()
                    .map(|token| token.player_request_po_token.as_str()),
            )
            .await
            .ok();
    }

    let mut used_age_restricted_client: Option<YouTubeClient> = None;
    let main_status = main_player_response
        .as_ref()
        .and_then(|response| response.playability_status.as_ref())
        .map(|status| status.status.clone())
        .unwrap_or_default();
    let is_age_restricted_from_response =
        RESTRICTED_STATUSES.contains(&main_status.as_str()) && !main_bot_checked;
    let was_originally_age_restricted = is_age_restricted_from_response;

    // restricted content signed in: WEB_CREATOR still serves it
    // (YTPlayerUtils.kt:481-491)
    if is_age_restricted_from_response
        && is_logged_in
        && let Ok(creator_response) = youtube
            .player(
                &request.video_id,
                request.playlist_id.as_deref(),
                &registry.get(ClientKey::WebCreator),
                None,
                None,
            )
            .await
        && creator_response
            .playability_status
            .as_ref()
            .is_some_and(|status| status.status == "OK")
    {
        used_age_restricted_client = Some(registry.get(ClientKey::WebCreator));
        main_player_response = Some(creator_response);
    }

    // the merged metadata (YTPlayerUtils.kt:493-496)
    let audio_config = metadata_response
        .as_ref()
        .and_then(|response| response.player_config.as_ref())
        .map(|config| config.audio_config.clone())
        .or_else(|| {
            main_player_response
                .as_ref()
                .and_then(|response| response.player_config.as_ref())
                .map(|config| config.audio_config.clone())
        });
    let video_details = metadata_response
        .as_ref()
        .and_then(|response| response.video_details.clone())
        .or_else(|| {
            main_player_response
                .as_ref()
                .and_then(|response| response.video_details.clone())
        });
    let playback_tracking = if is_logged_in {
        metadata_response
            .as_ref()
            .and_then(|response| response.playback_tracking.clone())
    } else {
        main_player_response
            .as_ref()
            .and_then(|response| response.playback_tracking.clone())
    };

    let is_private_track = main_player_response
        .as_ref()
        .and_then(|response| response.video_details.as_ref())
        .and_then(|details| details.music_video_type.as_deref())
        .is_some_and(|kind| kind == "MUSIC_VIDEO_TYPE_PRIVATELY_OWNED_TRACK");

    // the status AFTER the WEB_CREATOR swap governs both the account
    // prefix and the age-restricted restart (YTPlayerUtils.kt:504-520:
    // `currentStatus` is read from the already-replaced
    // mainPlayerResponse — the 2026-11-21 audit fixed the port from
    // reading the pre-swap status, which spuriously prepended the
    // account clients after a successful swap)
    let current_status = main_player_response
        .as_ref()
        .and_then(|response| response.playability_status.as_ref())
        .map(|status| status.status.clone())
        .unwrap_or_default();
    let is_age_restricted =
        RESTRICTED_STATUSES.contains(&current_status.as_str()) && !main_bot_checked;

    // the main client's answer is reused at index -1, so it isn't
    // asked again as a fallback, and signed in the account's clients
    // come first when it couldn't serve (YTPlayerUtils.kt:514-520)
    let main_served = current_status == "OK";
    let mut fallback_clients: Vec<YouTubeClient> = Vec::new();
    if is_logged_in && !main_served {
        fallback_clients.extend(
            super::walk_keys::ACCOUNT_CLIENT_KEYS
                .iter()
                .map(|key| registry.get(*key)),
        );
    }
    fallback_clients.extend(
        super::walk_keys::STREAM_FALLBACK_KEYS
            .iter()
            .filter(|key| **key != super::walk_keys::MAIN_CLIENT_KEY)
            .map(|key| registry.get(*key)),
    );

    let start_index = if main_player_response.is_none() || is_private_track || is_age_restricted {
        0
    } else {
        // usize::MAX plays the role of upstream's -1: the main
        // client's answer is reused without being asked again
        usize::MAX
    };

    // the WEB_CREATOR-replaced answer is what index -1 reuses
    // (upstream: `retryMainPlayerResponse = if (usedAgeRestrictedClient != null) mainPlayerResponse else null`)
    let retry_main_player_response = if used_age_restricted_client.is_some() {
        main_player_response.clone()
    } else {
        None
    };

    let mut chosen: Option<ChosenStream> = None;
    for client_index in walk_indices(start_index, fallback_clients.len()) {
        let (client, stream_player_response) = if client_index == usize::MAX {
            // upstream: `usedAgeRestrictedClient ?: MAIN_CLIENT`
            // (YTPlayerUtils.kt:624-628) — after the WEB_CREATOR swap
            // the replayed answer belongs to WEB_CREATOR: its
            // origin/Referer pair is what googlevideo validates the
            // url against (the 2026-11-21 audit fixed the port from
            // always attributing the replay to the main client)
            let replay_client = used_age_restricted_client
                .clone()
                .unwrap_or_else(|| main_client.clone());
            match retry_main_player_response
                .clone()
                .or_else(|| main_player_response.clone())
            {
                Some(response) => (replay_client, Some(response)),
                None => continue, // unreachable while start_index's
                                  // invariant holds; total, not panicking
            }
        } else {
            let client = fallback_clients[client_index].clone();
            // skip loginRequired clients when signed out
            // (YTPlayerUtils.kt:542-546)
            if client.login_required && !is_logged_in {
                tracing::debug!(target: "mochy_stream",
                    client = %client.client_name,
                    "skipping: requires login but the user is not logged in");
                continue;
            }
            // the lazy PoToken mint for the web clients
            // (YTPlayerUtils.kt:548-555)
            if client.use_web_po_tokens
                && po_token.is_none()
                && let Some(session_id) = &session_id
            {
                po_token = engine
                    .potoken
                    .po_token_for(transport, &request.video_id, session_id)
                    .await;
            }
            let client_po_token = client
                .use_web_po_tokens
                .then(|| {
                    po_token
                        .as_ref()
                        .map(|token| token.player_request_po_token.clone())
                })
                .flatten();
            let client_sig_timestamp =
                if was_originally_age_restricted || !client.use_signature_timestamp {
                    None
                } else {
                    sts().await.timestamp
                };
            let result = youtube
                .player(
                    &request.video_id,
                    request.playlist_id.as_deref(),
                    &client,
                    client_sig_timestamp,
                    client_po_token.as_deref(),
                )
                .await;
            match result {
                Ok(response) => (client, Some(response)),
                Err(error) => {
                    // every client asks the same host: the rest would
                    // be refused too (YTPlayerUtils.kt:568-572)
                    if matches!(
                        error,
                        crate::error::InnertubeError::Net(mochy_net::NetError::YouTubeRateLimited)
                    ) {
                        return Err(StreamError::RateLimited);
                    }
                    failures.push(format!("{}: {}", client.client_name, short_reason(&error)));
                    (client, None)
                }
            }
        };

        let Some(response) = stream_player_response else {
            continue;
        };
        let status_ok = response
            .playability_status
            .as_ref()
            .map(|status| status.status == "OK")
            .unwrap_or(false);
        if !status_ok {
            let status = response
                .playability_status
                .as_ref()
                .map(|status| status.status.clone())
                .unwrap_or_else(|| "Unknown".into());
            let reason = response
                .playability_status
                .as_ref()
                .and_then(|status| status.reason.clone())
                .unwrap_or_else(|| "No reason".into());
            tracing::debug!(target: "mochy_stream", client = %client.client_name, %status, %reason, "player response status not ok");
            // a request that failed outright was noted where it was
            // made (YTPlayerUtils.kt:719-724)
            failures.push(format!("{}: {} ({})", client.client_name, status, reason));
            if is_premium_only(&response) {
                premium_only = true;
            }
            continue;
        }
        any_client_served = true;

        // format candidates, then the first URL that resolves
        // (YTPlayerUtils.kt:593-620)
        let candidates = select_audio_format_candidates(&response, &request.conditions);
        if candidates.is_empty() {
            tracing::debug!(target: "mochy_stream", client = %client.client_name, "no suitable format found");
            continue;
        }
        let mut format = None;
        let mut stream_url = None;
        for candidate in &candidates {
            if let Some(url) = find_url_or_null(
                youtube,
                engine,
                candidate,
                &response,
                was_originally_age_restricted,
            )
            .await
            {
                format = Some(candidate.clone());
                stream_url = Some(url);
                break;
            }
            tracing::debug!(target: "mochy_stream",
                mime = %candidate.mime_type, bitrate = candidate.bitrate,
                "could not resolve the url, trying the next candidate");
        }
        let (Some(format), Some(mut stream_url)) = (format, stream_url) else {
            tracing::debug!(target: "mochy_stream", client = %client.client_name, "stream url not found for any candidate format");
            continue;
        };

        // the n-transform for the clients whose links carry one
        // (YTPlayerUtils.kt:632-643)
        if solves_n(&client)
            && let Some(solver) = engine.ejs_solver(transport).await?
        {
            let transformed = solver.transform_n_param_in_url(&stream_url).await;
            if transformed != stream_url {
                stream_url = transformed;
            }
        }

        // the per-video PoToken rides the URL as pot=
        // (YTPlayerUtils.kt:645-649)
        if client.use_web_po_tokens
            && let Some(token) = po_token
                .as_ref()
                .map(|token| token.streaming_data_po_token.clone())
        {
            let separator = if stream_url.contains('?') { "&" } else { "?" };
            stream_url = format!(
                "{stream_url}{separator}pot={}",
                super::urlutil::uri_encode(&token)
            );
        }

        let stream_expires_in_seconds = response
            .streaming_data
            .as_ref()
            .and_then(|data| data.expires_in_seconds);
        let Some(stream_expires_in_seconds) = stream_expires_in_seconds else {
            tracing::debug!(target: "mochy_stream", "stream expiration time not found");
            continue;
        };
        let expires = i32::try_from(stream_expires_in_seconds).unwrap_or(i32::MAX);

        // The last client used to be taken untested too, as better
        // than nothing. It isn't: a cut-off url plays ~30 seconds,
        // reaches READY and resets the skip counter (YTPlayerUtils.kt:
        // 664-672).
        let is_privately_owned = response
            .video_details
            .as_ref()
            .and_then(|details| details.music_video_type.as_deref())
            .is_some_and(|kind| kind == "MUSIC_VIDEO_TYPE_PRIVATELY_OWNED_TRACK");
        if is_privately_owned {
            chosen = Some(ChosenStream {
                client: client.clone(),
                format,
                stream_url,
                expires_in_seconds: expires,
            });
            break;
        }

        let headers = stream_headers(&client);
        match validate_stream_url(
            transport,
            &stream_url,
            &headers,
            format.content_length.map(|length| length as u64),
        )
        .await
        {
            ProbeOutcome::Served => {
                chosen = Some(ChosenStream {
                    client: client.clone(),
                    format,
                    stream_url,
                    expires_in_seconds: expires,
                });
                break;
            }
            ProbeOutcome::CutOff => {
                // one retry through the cipher's own n-transform before
                // giving up on the client (YTPlayerUtils.kt:691-713)
                let mut rescued = false;
                if solves_n(&client) {
                    let transformed = engine
                        .cipher
                        .transform_n_param_in_url(transport, &stream_url)
                        .await;
                    if transformed != stream_url {
                        let headers = stream_headers(&client);
                        if validate_stream_url(
                            transport,
                            &transformed,
                            &headers,
                            format.content_length.map(|length| length as u64),
                        )
                        .await
                            == ProbeOutcome::Served
                        {
                            stream_url = transformed;
                            chosen = Some(ChosenStream {
                                client: client.clone(),
                                format,
                                stream_url,
                                expires_in_seconds: expires,
                            });
                            rescued = true;
                        }
                    }
                }
                if rescued {
                    break;
                }
                failures.push(format!("{}: stream cut off", client.client_name));
            }
        }
    }

    // Every client failed. The error used to describe only the last
    // one tried — MOBILE, whose request failing outright came out as
    // "Bad stream player response" — so the reason the song couldn't
    // play never reached the user (YTPlayerUtils.kt:730-752).
    let Some(chosen) = chosen else {
        let summary = failure_summary(&failures);
        let failure = if !any_client_served && premium_only {
            StreamError::PremiumOnly(summary)
        } else if !any_client_served && bot_checked {
            StreamError::BotCheck(summary)
        } else {
            StreamError::Unplayable(summary)
        };
        // a user's own upload is never swapped for someone else's
        // copy, and a bot check would turn the stand-ins away too
        if !any_client_served
            && !request.is_uploaded_track()
            && !is_private_track
            && !matches!(failure, StreamError::BotCheck(_))
        {
            return Err(StreamError::UploadRefused {
                failure: Box::new(failure),
                video_details: main_player_response
                    .as_ref()
                    .and_then(|response| response.video_details.clone())
                    .map(Box::new),
            });
        }
        return Err(failure);
    };

    let data = PlaybackData::from_parts(
        audio_config,
        video_details,
        playback_tracking,
        chosen.format,
        chosen.stream_url,
        chosen.expires_in_seconds,
        &chosen.client.client_name,
        stream_headers(&chosen.client),
    );
    // remember the resolution; the audio layer's 403 invalidates it
    engine.cache.lock().await.put(
        &request.video_id,
        data.stream_url.clone(),
        data.format.clone(),
        data.stream_expires_in_seconds as i64,
    );
    Ok(data)
}

/// The chosen stream of one successful client iteration.
struct ChosenStream {
    client: YouTubeClient,
    format: crate::models::response::player::Format,
    stream_url: String,
    expires_in_seconds: i32,
}

/// The walk's client indices: upstream's `-1 until size` loop, with
/// `usize::MAX` playing the `-1` (the reused main answer).
fn walk_indices(start_index: usize, fallback_len: usize) -> impl Iterator<Item = usize> {
    let main_first = start_index == usize::MAX;
    let range = 0..fallback_len;
    main_first.then_some(usize::MAX).into_iter().chain(range)
}

/// `askMainClient` (YTPlayerUtils.kt:403-410): a failure here fails
/// the whole walk — a network-level failure on the main client is
/// not retried against the other clients.
async fn ask_main(
    youtube: &YouTube,
    request: &PlaybackRequest,
    client: &YouTubeClient,
    po_token: Option<&str>,
    signature_timestamp: Option<i32>,
) -> Result<PlayerResponse, StreamError> {
    youtube
        .player(
            &request.video_id,
            request.playlist_id.as_deref(),
            client,
            signature_timestamp,
            po_token,
        )
        .await
        .map_err(|error| main_client_failure(&error))
}

/// `shortReason` (YTPlayerUtils.kt:1220-1225): Ktor's own message
/// carries the whole request url and response body.
fn short_reason(error: &crate::error::InnertubeError) -> String {
    match error {
        crate::error::InnertubeError::Net(mochy_net::NetError::Http { status, .. }) => {
            format!("HTTP {}", status.as_u16())
        }
        crate::error::InnertubeError::Net(mochy_net::NetError::Io(_)) => "timed out".into(),
        other => {
            let text = other.to_string();
            text.lines().next().unwrap_or_default().to_owned()
        }
    }
}

/// `failureSummary` (YTPlayerUtils.kt:1205-1214): one line for the
/// notification — the first client's reason, which is the main
/// client's.
fn failure_summary(failures: &[String]) -> String {
    let Some(first) = failures.first() else {
        return "YouTube didn't return a playable stream".into();
    };
    let others = failures.len() - 1;
    if others > 0 {
        format!("YouTube wouldn't stream this song ({first}; {others} other clients failed too)")
    } else {
        format!("YouTube wouldn't stream this song ({first})")
    }
}

/// `findUrlOrNull` (YTPlayerUtils.kt:1129-1187): the direct url, the
/// cipher deobfuscation, then the NewPipe legs.
async fn find_url_or_null(
    youtube: &YouTube,
    engine: &StreamEngine,
    format: &crate::models::response::player::Format,
    player_response: &PlayerResponse,
    skip_newpipe: bool,
) -> Option<String> {
    let transport = youtube.transport();
    if format.url.as_deref().is_some_and(|url| !url.is_empty()) {
        return format.url.clone();
    }
    let signature_cipher = format
        .signature_cipher
        .clone()
        .or_else(|| format.cipher.clone())?;
    if let Some(deobfuscated) = engine
        .cipher
        .deobfuscate_stream_url(
            transport,
            &signature_cipher,
            &youtube_id(player_response, format),
        )
        .await
    {
        return Some(deobfuscated);
    }
    if skip_newpipe {
        return None;
    }
    // NewPipe's getStreamUrl over the port's own cipher: the same
    // functions, the same player script
    // (NewPipe.kt:107-140)
    if let Some(deobfuscated) = newpipe_get_stream_url(youtube, engine, &signature_cipher).await {
        return Some(deobfuscated);
    }
    // the StreamInfo last resort: the watch page's own urls
    // (YTPlayerUtils.kt:1164-1183)
    let video_id = youtube_id(player_response, format);
    let stream_urls = newpipe::watch_page_stream_urls(transport, &video_id)
        .await
        .ok()?;
    newpipe::pick_stream_url(&stream_urls, format, player_response)
}

/// The video id a format belongs to (the details carry it; the
/// cipher only logs it).
fn youtube_id(
    player_response: &PlayerResponse,
    _format: &crate::models::response::player::Format,
) -> String {
    player_response
        .video_details
        .as_ref()
        .map(|details: &VideoDetails| details.video_id.clone())
        .unwrap_or_default()
}

/// NewPipe.kt:107-140 — the signatureCipher path plus the throttling
/// (`n`) deobfuscation, over the port's cipher.
async fn newpipe_get_stream_url(
    youtube: &YouTube,
    engine: &StreamEngine,
    signature_cipher: &str,
) -> Option<String> {
    let transport = youtube.transport();
    let params = super::cipher::parse_query_params(signature_cipher);
    let _obfuscated_signature = params.get("s")?;
    let deobfuscated = engine
        .cipher
        .deobfuscate_stream_url(transport, signature_cipher, "")
        .await?;
    // the throttling parameter rides the deobfuscated url
    let with_n = engine
        .cipher
        .transform_n_param_in_url(transport, &deobfuscated)
        .await;
    Some(with_n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_check_and_premium_only_match_the_words() {
        let mut response = PlayerResponse::default();
        response.playability_status = Some(crate::models::response::player::PlayabilityStatus {
            status: "LOGIN_REQUIRED".into(),
            reason: Some("Please sign in to confirm you're not a bot".into()),
        });
        assert!(is_bot_check(&response));
        assert!(!is_premium_only(&response));
        response.playability_status = Some(crate::models::response::player::PlayabilityStatus {
            status: "LOGIN_REQUIRED".into(),
            reason: Some("This video is available to Music Premium members".into()),
        });
        assert!(!is_bot_check(&response));
        assert!(is_premium_only(&response));
        response.playability_status = Some(crate::models::response::player::PlayabilityStatus {
            status: "OK".into(),
            reason: Some("premium in the reason but status ok".into()),
        });
        assert!(
            !is_premium_only(&response),
            "an OK status is never premium-only"
        );
    }

    #[test]
    fn walk_indices_lead_with_the_main_reuse_slot() {
        let indices: Vec<usize> = walk_indices(usize::MAX, 3).collect();
        assert_eq!(indices, vec![usize::MAX, 0, 1, 2]);
        let indices: Vec<usize> = walk_indices(0, 2).collect();
        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn failure_summary_names_the_first_client_and_the_count() {
        assert_eq!(
            failure_summary(&["VISIONOS: LOGIN_REQUIRED (bot)".into()]),
            "YouTube wouldn't stream this song (VISIONOS: LOGIN_REQUIRED (bot))"
        );
        assert_eq!(
            failure_summary(&[
                "VISIONOS: LOGIN_REQUIRED (bot)".into(),
                "WEB_REMIX: stream cut off".into()
            ]),
            "YouTube wouldn't stream this song (VISIONOS: LOGIN_REQUIRED (bot); 1 other clients failed too)"
        );
        assert_eq!(
            failure_summary(&[]),
            "YouTube didn't return a playable stream"
        );
    }

    #[test]
    fn short_reason_trims_to_one_line() {
        let error = crate::error::InnertubeError::Net(mochy_net::NetError::Http {
            status: reqwest::StatusCode::BAD_REQUEST,
            url: "https://example.com".into(),
        });
        assert_eq!(short_reason(&error), "HTTP 400");
    }
}
