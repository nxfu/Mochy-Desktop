//! The endpoint layer — all 22 endpoints of the innertube module
//! (blueprint ch. 4.1), ported from `InnerTube.kt:225-835` and the three
//! app-side dedicated clients (`YouTube.registerPlayback` /
//! `InnerTube.returnYouTubeDislike` / `PoTokenWebView`'s BotGuard
//! service requests).
//!
//! Phase 04 returns the raw `serde_json::Value` (or `String` for the
//! non-JSON payloads); Phase 05's parsers + facade consume these and
//! produce the typed pages. Every method cites its upstream lines.
//!
//! The endpoint catalog:
//!
//! | # | endpoint | upstream |
//! |---|----------|----------|
//! | 1 | `search` | InnerTube.kt:225-247 |
//! | 2 | `browse` | InnerTube.kt:309-333 |
//! | 3 | `player` | InnerTube.kt:249-285 |
//! | 4 | `next` | InnerTube.kt:335-358 |
//! | 5 | `feedback` | InnerTube.kt:360-373 |
//! | 6 | `music/get_search_suggestions` | InnerTube.kt:375-388 |
//! | 7 | `music/get_queue` | InnerTube.kt:390-405 |
//! | 8 | `get_transcript` | InnerTube.kt:407-424 |
//! | 9 | `visitor_id` | InnerTube.kt:184-202 |
//! | 10 | `account/account_menu` | InnerTube.kt:436-441 |
//! | 11 | `like/like` | InnerTube.kt:443-456 + 503-516 |
//! | 12 | `like/removelike` | InnerTube.kt:458-471 + 518-531 |
//! | 13 | `subscription/subscribe` | InnerTube.kt:473-486 |
//! | 14 | `subscription/unsubscribe` | InnerTube.kt:488-501 |
//! | 15 | `browse/edit_playlist` (7 actions) | InnerTube.kt:533-632 + 649-668 + 701-740 |
//! | 16 | `playlist/create` | InnerTube.kt:634-647 |
//! | 17 | `playlist/delete` | InnerTube.kt:742-756 |
//! | 18 | `playlist_image_upload` (resumable, 2 steps) | InnerTube.kt:670-699 |
//! | 19 | `timedtext` | InnerTube.kt:427-432 |
//! | 20 | `sw.js_data` | InnerTube.kt:434 |
//! | 21 | videostats play reporting | InnerTube.kt:287-307 + YouTube.kt:1270-1300 |
//! | 22 | ReturnYouTubeDislike Votes + BotGuard Create/GenerateIT | InnerTube.kt:758-762 + PoTokenWebView.kt:97-165 |

use base64::Engine as _;
use serde_json::Value as Json;

use crate::body::privacy_private;
use crate::body::*;
use crate::client::{ClientKey, ClientRegistry, ORIGIN_YOUTUBE_MUSIC, YouTubeClient};
use crate::error::InnertubeError;
use crate::transport::{InnerTubeTransport, ShapedRequest};

use mochy_net::NetError;

/// The endpoint name catalog — the task's 22-entry enumeration
/// (blueprint ch. 4.1), used by the fixture harness keys and the
/// endpoint-count test. `subscription/subscribe + subscription/unsubscribe`
/// is one entry (one subscription surface, two actions), which is how the
/// upstream task text counts them; the transport exposes both methods.
pub const ENDPOINTS: &[&str] = &[
    "search",
    "browse",
    "player",
    "next",
    "feedback",
    "music/get_search_suggestions",
    "music/get_queue",
    "get_transcript",
    "visitor_id",
    "account/account_menu",
    "like/like",
    "like/removelike",
    "subscription/subscribe+unsubscribe",
    "browse/edit_playlist",
    "playlist/create",
    "playlist/delete",
    "playlist_image_upload",
    "timedtext",
    "sw.js_data",
    "videostats_play_reporting",
    "returnyoutubedislike_votes",
    "botguard_create_generate_it",
];

/// The BotGuard service constants (PoTokenWebView.kt:301-306).
pub const BOTGUARD_API_KEY: &str = "AIzaSyDyT5W0Jh49F30Pqqtyfdf7pDLFKLJoAnw";
pub const BOTGUARD_REQUEST_KEY: &str = "O43z0dpjhgX20SCx4KAo";
pub const BOTGUARD_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.3";

impl InnerTubeTransport {
    /// `search` (InnerTube.kt:225-247): continuation rides the query
    /// string (`continuation` + `ctoken`), login headers only when
    /// `useLoginForBrowse` is on.
    pub async fn search(
        &self,
        client: &YouTubeClient,
        query: Option<&str>,
        params: Option<&str>,
        continuation: Option<&str>,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = SearchBody {
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session
                    .use_login_for_browse
                    .then_some(session.data_sync_id.as_deref())
                    .flatten(),
            ),
            query: query.map(str::to_owned),
            params: params.map(str::to_owned),
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "search",
            client,
            session.use_login_for_browse,
            session.visitor_data.clone(),
        );
        if let Some(continuation) = continuation {
            request
                .query
                .push(("continuation".into(), continuation.into()));
            request.query.push(("ctoken".into(), continuation.into()));
        }
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `browse` (InnerTube.kt:309-333): `setLogin || useLoginForBrowse`
    /// decides both the headers and whether `onBehalfOfUser` rides the
    /// context; `hlOverride` forces the response language.
    pub async fn browse(
        &self,
        client: &YouTubeClient,
        browse_id: Option<&str>,
        params: Option<&str>,
        continuation: Option<&str>,
        set_login: bool,
        hl_override: Option<&str>,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let login = set_login || session.use_login_for_browse;
        let mut locale = session.locale.clone();
        if let Some(hl) = hl_override {
            locale.hl = hl.to_owned();
        }
        let body = BrowseBody {
            context: client.to_context(
                &locale,
                session.visitor_data.as_deref(),
                login.then_some(session.data_sync_id.as_deref()).flatten(),
            ),
            browse_id: browse_id.map(str::to_owned),
            params: params.map(str::to_owned),
            continuation: continuation.map(str::to_owned),
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "browse",
            client,
            login,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `player` (InnerTube.kt:249-285): always `setLogin = true`;
    /// ownVisitorId clients carry their minted id; embedded clients add
    /// the `thirdParty.embedUrl`; STS rides `playbackContext` only for
    /// `useSignatureTimestamp` clients; the PO token rides
    /// `serviceIntegrityDimensions` only for `useWebPoTokens` clients.
    pub async fn player(
        &self,
        client: &YouTubeClient,
        video_id: &str,
        playlist_id: Option<&str>,
        signature_timestamp: Option<i32>,
        po_token: Option<&str>,
    ) -> Result<Json, InnertubeError> {
        let visitor = crate::transport::resolve_visitor(self, client).await?;
        let session = self.session_snapshot()?;
        let mut context = client.to_context(
            &session.locale,
            visitor.as_deref(),
            session.data_sync_id.as_deref(),
        );
        if client.is_embedded {
            context.third_party = Some(crate::context::ThirdParty {
                embed_url: format!("https://www.youtube.com/watch?v={video_id}"),
            });
        }
        let body = PlayerBody {
            context,
            video_id: video_id.to_owned(),
            playlist_id: playlist_id.map(str::to_owned),
            playback_context: match (client.use_signature_timestamp, signature_timestamp) {
                (true, Some(sts)) => Some(PlaybackContext {
                    content_playback_context: ContentPlaybackContext::new(sts),
                }),
                _ => None,
            },
            service_integrity_dimensions: match (client.use_web_po_tokens, po_token) {
                (true, Some(token)) => Some(ServiceIntegrityDimensions {
                    po_token: token.to_owned(),
                }),
                _ => None,
            },
            content_check_ok: true,
            racy_check_ok: true,
        };
        // the request goes to the client's own origin (InnerTube.kt:257)
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "player",
            client,
            true,
            visitor.clone(),
        );
        request.url = format!("{}/youtubei/v1/player", client.api_origin);
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `next` (InnerTube.kt:335-358).
    #[allow(clippy::too_many_arguments)]
    pub async fn next(
        &self,
        client: &YouTubeClient,
        video_id: Option<&str>,
        playlist_id: Option<&str>,
        playlist_set_video_id: Option<&str>,
        index: Option<i32>,
        params: Option<&str>,
        continuation: Option<&str>,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = NextBody {
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session.data_sync_id.as_deref(),
            ),
            video_id: video_id.map(str::to_owned),
            playlist_id: playlist_id.map(str::to_owned),
            playlist_set_video_id: playlist_set_video_id.map(str::to_owned),
            index,
            params: params.map(str::to_owned),
            continuation: continuation.map(str::to_owned),
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "next",
            client,
            true,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `feedback` (InnerTube.kt:360-373).
    pub async fn feedback(
        &self,
        client: &YouTubeClient,
        tokens: &[String],
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = FeedbackBody {
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session.data_sync_id.as_deref(),
            ),
            feedback_tokens: tokens.to_vec(),
            is_feedback_token_unencrypted: false,
            should_merge: false,
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "feedback",
            client,
            true,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `music/get_search_suggestions` (InnerTube.kt:375-388) — no login
    /// headers, no dataSyncId in the context.
    pub async fn get_search_suggestions(
        &self,
        client: &YouTubeClient,
        input: &str,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = GetSearchSuggestionsBody {
            context: client.to_context(&session.locale, session.visitor_data.as_deref(), None),
            input: input.to_owned(),
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "music/get_search_suggestions",
            client,
            false,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `music/get_queue` (InnerTube.kt:390-405).
    pub async fn get_queue(
        &self,
        client: &YouTubeClient,
        video_ids: Option<&[String]>,
        playlist_id: Option<&str>,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = GetQueueBody {
            context: client.to_context(&session.locale, session.visitor_data.as_deref(), None),
            video_ids: video_ids.map(|ids| ids.to_vec()),
            playlist_id: playlist_id.map(str::to_owned),
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "music/get_queue",
            client,
            false,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `get_transcript` (InnerTube.kt:407-424): hardcoded music origin,
    /// only a Content-Type header (no client headers, no visitor), the
    /// params envelope is `base64("\n\x0b{videoId}")`.
    pub async fn get_transcript(
        &self,
        client: &YouTubeClient,
        video_id: &str,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = GetTranscriptBody {
            context: client.to_context(&session.locale, None, None),
            params: transcript_params(video_id),
        };
        let mut request =
            ShapedRequest::endpoint(reqwest::Method::POST, "get_transcript", client, false, None);
        request.url = format!("{ORIGIN_YOUTUBE_MUSIC}/youtubei/v1/get_transcript");
        // InnerTube.kt:407-424 — only a Content-Type header rides this
        // request (no client headers, no visitor); the body's own JSON
        // serialization provides the content type
        request.use_client_headers = false;
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `visitor_id` (InnerTube.kt:184-202): a fresh visitor id minted as
    /// `client`, the way the app gets one before its first player
    /// request. The response's `responseContext.visitorData` is extracted
    /// and cached in the session's `own_visitor_ids`.
    pub async fn visitor_id(&self, client: &YouTubeClient) -> Result<String, InnertubeError> {
        mint_visitor_id(self, client).await
    }

    /// `account/account_menu` (InnerTube.kt:436-441).
    pub async fn account_menu(&self, client: &YouTubeClient) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = AccountMenuBody {
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session.data_sync_id.as_deref(),
            ),
            device_theme: "DEVICE_THEME_SELECTED".into(),
            user_interface_theme: "USER_INTERFACE_THEME_DARK".into(),
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "account/account_menu",
            client,
            true,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `like/like` for a video (InnerTube.kt:443-456).
    pub async fn like_video(
        &self,
        client: &YouTubeClient,
        video_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.like(
            client,
            LikeTarget::Video {
                video_id: video_id.into(),
            },
        )
        .await
    }

    /// `like/removelike` for a video (InnerTube.kt:458-471).
    pub async fn unlike_video(
        &self,
        client: &YouTubeClient,
        video_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.remove_like(
            client,
            LikeTarget::Video {
                video_id: video_id.into(),
            },
        )
        .await
    }

    /// `like/like` for a playlist (InnerTube.kt:503-516).
    pub async fn like_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.like(
            client,
            LikeTarget::Playlist {
                playlist_id: playlist_id.into(),
            },
        )
        .await
    }

    /// `like/removelike` for a playlist (InnerTube.kt:518-531).
    pub async fn unlike_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.remove_like(
            client,
            LikeTarget::Playlist {
                playlist_id: playlist_id.into(),
            },
        )
        .await
    }

    async fn like(
        &self,
        client: &YouTubeClient,
        target: LikeTarget,
    ) -> Result<Json, InnertubeError> {
        self.like_endpoint("like/like", client, target).await
    }

    async fn remove_like(
        &self,
        client: &YouTubeClient,
        target: LikeTarget,
    ) -> Result<Json, InnertubeError> {
        self.like_endpoint("like/removelike", client, target).await
    }

    async fn like_endpoint(
        &self,
        endpoint: &str,
        client: &YouTubeClient,
        target: LikeTarget,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = LikeBody {
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session.data_sync_id.as_deref(),
            ),
            target,
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            endpoint,
            client,
            true,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `subscription/subscribe` (InnerTube.kt:473-486).
    pub async fn subscribe_channel(
        &self,
        client: &YouTubeClient,
        channel_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.subscribe_endpoint("subscription/subscribe", client, channel_id)
            .await
    }

    /// `subscription/unsubscribe` (InnerTube.kt:488-501).
    pub async fn unsubscribe_channel(
        &self,
        client: &YouTubeClient,
        channel_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.subscribe_endpoint("subscription/unsubscribe", client, channel_id)
            .await
    }

    async fn subscribe_endpoint(
        &self,
        endpoint: &str,
        client: &YouTubeClient,
        channel_id: &str,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = SubscribeBody {
            channel_ids: vec![channel_id.to_owned()],
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session.data_sync_id.as_deref(),
            ),
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            endpoint,
            client,
            true,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `browse/edit_playlist` with any of the 7 actions
    /// (InnerTube.kt:533-632, 649-668, 701-740).
    pub async fn edit_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        actions: Vec<EditAction>,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = EditPlaylistBody {
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session.data_sync_id.as_deref(),
            ),
            playlist_id: playlist_id.to_owned(),
            actions,
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "browse/edit_playlist",
            client,
            true,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `addToPlaylist` (InnerTube.kt:533-550) — strips the `VL` prefix.
    pub async fn add_to_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        video_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.edit_playlist(
            client,
            strip_vl_prefix(playlist_id),
            vec![EditAction::add_video(video_id)],
        )
        .await
    }

    /// `addVideosToPlaylist` (InnerTube.kt:552-567).
    pub async fn add_videos_to_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        video_ids: &[String],
    ) -> Result<Json, InnertubeError> {
        self.edit_playlist(
            client,
            strip_vl_prefix(playlist_id),
            video_ids
                .iter()
                .map(|id| EditAction::add_video(id.clone()))
                .collect(),
        )
        .await
    }

    /// `addPlaylistToPlaylist` (InnerTube.kt:569-586).
    pub async fn add_playlist_to_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        add_playlist_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.edit_playlist(
            client,
            strip_vl_prefix(playlist_id),
            vec![EditAction::add_playlist(add_playlist_id)],
        )
        .await
    }

    /// `removeFromPlaylist` (InnerTube.kt:588-609).
    pub async fn remove_from_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        video_id: &str,
        set_video_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.edit_playlist(
            client,
            strip_vl_prefix(playlist_id),
            vec![EditAction::remove_video(set_video_id, video_id)],
        )
        .await
    }

    /// `moveSongPlaylist` (InnerTube.kt:611-632) — no VL strip.
    pub async fn move_song_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        set_video_id: &str,
        successor_set_video_id: Option<&str>,
    ) -> Result<Json, InnertubeError> {
        self.edit_playlist(
            client,
            playlist_id,
            vec![EditAction::move_video(
                set_video_id,
                successor_set_video_id.map(str::to_owned),
            )],
        )
        .await
    }

    /// `renamePlaylist` (InnerTube.kt:649-668) — no VL strip.
    pub async fn rename_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        name: &str,
    ) -> Result<Json, InnertubeError> {
        self.edit_playlist(client, playlist_id, vec![EditAction::rename_playlist(name)])
            .await
    }

    /// `setThumbnailPlaylist` (InnerTube.kt:701-722) — no VL strip.
    pub async fn set_thumbnail_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        blob_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.edit_playlist(
            client,
            playlist_id,
            vec![EditAction::set_custom_thumbnail(blob_id)],
        )
        .await
    }

    /// `removeThumbnailPlaylist` (InnerTube.kt:724-740) — no VL strip.
    pub async fn remove_thumbnail_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
    ) -> Result<Json, InnertubeError> {
        self.edit_playlist(
            client,
            playlist_id,
            vec![EditAction::remove_custom_thumbnail()],
        )
        .await
    }

    /// `playlist/create` (InnerTube.kt:634-647).
    pub async fn create_playlist(
        &self,
        client: &YouTubeClient,
        title: &str,
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let body = CreatePlaylistBody {
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session.data_sync_id.as_deref(),
            ),
            title: title.to_owned(),
            privacy_status: privacy_private(),
            video_ids: None,
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "playlist/create",
            client,
            true,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `playlist/delete` (InnerTube.kt:742-756). (Upstream logs
    /// "deleting $playlistId" via println — debug-build noise the port
    /// routes through tracing at debug level instead.)
    pub async fn delete_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
    ) -> Result<Json, InnertubeError> {
        tracing::debug!(playlist_id, "deleting playlist");
        let session = self.session_snapshot()?;
        let body = PlaylistDeleteBody {
            context: client.to_context(
                &session.locale,
                session.visitor_data.as_deref(),
                session.data_sync_id.as_deref(),
            ),
            playlist_id: playlist_id.to_owned(),
        };
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "playlist/delete",
            client,
            true,
            session.visitor_data.clone(),
        );
        request.body = Some(serde_json::to_value(&body)?);
        self.send_json(request).await
    }

    /// `playlist_image_upload` step 1 (InnerTube.kt:670-682): the
    /// resumable start. Returns the `x-guploader-uploadid` response
    /// header value.
    pub async fn playlist_image_upload_start(
        &self,
        client: &YouTubeClient,
        content_length: usize,
    ) -> Result<String, InnertubeError> {
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "playlist_image_upload/playlist_custom_thumbnail",
            client,
            true,
            self.session_snapshot()?.visitor_data.clone(),
        );
        request.url =
            format!("{ORIGIN_YOUTUBE_MUSIC}/playlist_image_upload/playlist_custom_thumbnail");
        request
            .headers
            .push(("X-Goog-Upload-Command".into(), "start".into()));
        request
            .headers
            .push(("X-Goog-Upload-Protocol".into(), "resumable".into()));
        request.headers.push((
            "X-Goog-Upload-Header-Content-Length".into(),
            content_length.to_string(),
        ));
        request.capture_headers = vec!["x-guploader-uploadid".into()];
        self.send_json(request).await?;
        let captured = self.take_captured_headers()?;
        captured
            .get("x-guploader-uploadid")
            .cloned()
            .ok_or(InnertubeError::UploadIdMissing)
    }

    /// `playlist_image_upload` step 2 (InnerTube.kt:684-699): the
    /// upload+finalize with the raw image body — `setBody(image)` in
    /// ktor sends the untouched bytes (defaulting the content type to
    /// application/octet-stream); the 2026-11-21 audit fixed this from
    /// a lossy JSON-string body that corrupted every non-ASCII byte.
    pub async fn playlist_image_upload_finalize(
        &self,
        client: &YouTubeClient,
        upload_id: &str,
        image: Vec<u8>,
    ) -> Result<Json, InnertubeError> {
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::POST,
            "playlist_image_upload/playlist_custom_thumbnail",
            client,
            true,
            self.session_snapshot()?.visitor_data.clone(),
        );
        request.url =
            format!("{ORIGIN_YOUTUBE_MUSIC}/playlist_image_upload/playlist_custom_thumbnail");
        request
            .query
            .push(("upload_id".into(), upload_id.to_owned()));
        request
            .query
            .push(("upload_protocol".into(), "resumable".into()));
        request
            .headers
            .push(("X-Goog-Upload-Command".into(), "upload, finalize".into()));
        request
            .headers
            .push(("X-Goog-Upload-Offset".into(), "0".into()));
        request.raw_body = Some(image);
        self.send_json(request).await
    }

    /// `timedtext` (InnerTube.kt:427-432): GET `baseUrl` with `fmt=json3`
    /// and the client's user agent — the one header ktor's `timedText`
    /// adds on top of the shared defaults (no X-YouTube-Client-* set,
    /// no prettyPrint; the 2026-11-21 audit restored this).
    pub async fn timed_text(
        &self,
        base_url: &str,
        client: &YouTubeClient,
    ) -> Result<Json, InnertubeError> {
        let mut request =
            ShapedRequest::endpoint(reqwest::Method::GET, "timedtext", client, false, None);
        request.url = base_url.to_owned();
        request.use_client_headers = false;
        request
            .headers
            .push(("User-Agent".into(), client.user_agent.clone()));
        request.query.push(("fmt".into(), "json3".into()));
        request.cacheable = true;
        self.send_json(request).await
    }

    /// `sw.js_data` (InnerTube.kt:434): a plain GET to
    /// `https://music.youtube.com/sw.js_data`, no client headers. The
    /// payload is NOT JSON at the top level (it starts with `)]}'`).
    pub async fn sw_js_data(&self) -> Result<String, InnertubeError> {
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::GET,
            "sw.js_data",
            &ClientRegistry::new().get(ClientKey::Web),
            false,
            None,
        );
        request.url = format!("{ORIGIN_YOUTUBE_MUSIC}/sw.js_data");
        request.use_client_headers = false;
        request.cacheable = true;
        self.send_text(request).await
    }

    /// videostats play reporting (InnerTube.kt:287-307 + the facade's
    /// `musicStatsUrl`/`newPlaybackNonce`, YouTube.kt:1266-1300): GET the
    /// tracking url as `client` with `ver`/`c`/`cver`/`cpn` params, the
    /// playlist `list`/`referrer` pair, and any extra params the
    /// watchtime reporter carries.
    pub async fn register_playback(
        &self,
        url: &str,
        cpn: &str,
        playlist_id: Option<&str>,
        client: &YouTubeClient,
        extra_params: &[(String, String)],
    ) -> Result<Json, InnertubeError> {
        let session = self.session_snapshot()?;
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::GET,
            "videostats",
            client,
            true,
            session.visitor_data.clone(),
        );
        request.url = music_stats_url(url);
        request.query.push(("ver".into(), "2".into()));
        request.query.push(("c".into(), client.client_name.clone()));
        request
            .query
            .push(("cver".into(), client.client_version.clone()));
        request.query.push(("cpn".into(), cpn.to_owned()));
        for (name, value) in extra_params {
            request.query.push((name.clone(), value.clone()));
        }
        if let Some(playlist_id) = playlist_id {
            request.query.push(("list".into(), playlist_id.to_owned()));
            request.query.push((
                "referrer".into(),
                format!("https://music.youtube.com/playlist?list={playlist_id}"),
            ));
        }
        self.send_json(request).await
    }

    /// ReturnYouTubeDislike `Votes` (InnerTube.kt:758-762) — the small
    /// dedicated client on `returnyoutubedislikeapi.com`.
    pub async fn return_youtube_dislike_votes(
        &self,
        video_id: &str,
    ) -> Result<Json, InnertubeError> {
        let mut request = ShapedRequest::endpoint(
            reqwest::Method::GET,
            "returnyoutubedislike_votes",
            &ClientRegistry::new().get(ClientKey::Web),
            false,
            None,
        );
        request.url = format!("https://returnyoutubedislikeapi.com/Votes?videoId={video_id}");
        request.use_client_headers = false;
        request.json_content_type = true;
        request.cacheable = true;
        self.send_json(request).await
    }

    /// BotGuard `Create` (PoTokenWebView.kt:97-116) — the challenge
    /// fetch, a small dedicated client on
    /// `https://www.youtube.com/api/jnn/v1/Create` with the protobuf-ish
    /// JSON array body `[ "requestKey" ]`.
    pub async fn botguard_create(&self, request_key: &str) -> Result<String, InnertubeError> {
        self.botguard_service_request("Create", &format!("[ \"{request_key}\" ]"))
            .await
    }

    /// BotGuard `GenerateIT` (PoTokenWebView.kt:130-134) — the
    /// integrity-token exchange, body
    /// `[ "requestKey", "botguardResponse" ]`.
    pub async fn botguard_generate_it(
        &self,
        request_key: &str,
        botguard_response: &str,
    ) -> Result<String, InnertubeError> {
        self.botguard_service_request(
            "GenerateIT",
            &format!("[ \"{request_key}\", \"{botguard_response}\" ]"),
        )
        .await
    }

    /// The shared BotGuard request shape (PoTokenWebView.kt:251-280):
    /// POST the raw JSON array with the web API key headers.
    async fn botguard_service_request(
        &self,
        endpoint: &str,
        data: &str,
    ) -> Result<String, InnertubeError> {
        let client = self.ensure_client()?;
        let url = format!("https://www.youtube.com/api/jnn/v1/{endpoint}");
        let url = self.rewrite(url);
        let response = client
            .post(&url)
            .headers(botguard_headers())
            .body(data.to_owned())
            .send()
            .await
            .map_err(|e| InnertubeError::Net(NetError::from(e)))?;
        let status = response.status();
        // PoTokenWebView.kt:270-273 — non-200 is fatal for the engine
        // boot (upstream compares `httpCode != 200` exactly, not 2xx)
        if status.as_u16() != 200 {
            return Err(InnertubeError::Net(mochy_net::NetError::Http {
                status,
                url,
            }));
        }
        response
            .text()
            .await
            .map_err(|e| InnertubeError::Net(NetError::Other(e.to_string())))
    }
}

/// The BotGuard header set (PoTokenWebView.kt:259-265).
fn botguard_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    let mut insert = |name: &'static str, value: &str| {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::try_from(name),
            reqwest::header::HeaderValue::try_from(value),
        ) {
            headers.insert(name, value);
        }
    };
    insert("User-Agent", BOTGUARD_USER_AGENT);
    insert("Accept", "application/json");
    insert("Content-Type", "application/json+protobuf");
    insert("x-goog-api-key", BOTGUARD_API_KEY);
    insert("x-user-agent", "grpc-web-javascript/0.1");
    headers
}

/// `transcript_params` — `Base64.Default.encode("\n${11.toChar()}$videoId")`
/// (InnerTube.kt:418-420): the literal bytes `\n`, `\x0b`, then the id.
pub fn transcript_params(video_id: &str) -> String {
    let mut bytes = b"\n\x0b".to_vec();
    bytes.extend_from_slice(video_id.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(&bytes)
}

/// `musicStatsUrl` (YouTube.kt:1300) — the player hands out s.youtube.com
/// links, the music site takes the same ones for its own plays.
pub fn music_stats_url(url: &str) -> String {
    url.replace("https://s.youtube.com", "https://music.youtube.com")
}

/// `newPlaybackNonce` (YouTube.kt:1266-1268) — a fresh id for one listen;
/// youtube ties a song's play and its watch time together by it.
pub fn new_playback_nonce() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
    let mut nonce = String::with_capacity(16);
    let mut state = rand_state();
    for _ in 0..16 {
        // xorshift over the nanos seed — matches the spirit of
        // Random.Default.nextInt(0, 64): uniform over the 64 chars
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        nonce.push(ALPHABET[(state % 64) as usize] as char);
    }
    nonce
}

fn rand_state() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1
}

/// The visitor_id mint — InnerTube.kt:192-202. Public to the crate (the
/// transport's own-visitor-id resolution and the guest rotation call it).
pub(crate) async fn mint_visitor_id(
    transport: &InnerTubeTransport,
    client: &YouTubeClient,
) -> Result<String, InnertubeError> {
    let session = transport.session_snapshot()?;
    let body = VisitorIdBody {
        context: client.to_context(&session.locale, None, None),
    };
    let mut request =
        ShapedRequest::endpoint(reqwest::Method::POST, "visitor_id", client, false, None);
    request.url = format!("{}/youtubei/v1/visitor_id", client.api_origin);
    request.body = Some(serde_json::to_value(&body)?);
    let answer = transport.send_json(request).await?;
    let visitor = answer
        .get("responseContext")
        .and_then(|ctx| ctx.get("visitorData"))
        .and_then(|v| v.as_str())
        .ok_or(InnertubeError::VisitorIdMissing)?
        .to_owned();
    transport
        .session()
        .write()
        .map_err(|_| InnertubeError::LockPoisoned)?
        .own_visitor_ids
        .insert(client.client_name.clone(), visitor.clone());
    Ok(visitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_catalog_has_the_documented_22() {
        assert_eq!(ENDPOINTS.len(), 22);
        // the YouTube-side paths (RYD + BotGuard ride their own hosts)
        assert!(ENDPOINTS.contains(&"search"));
        assert!(ENDPOINTS.contains(&"browse"));
        assert!(ENDPOINTS.contains(&"player"));
        assert!(ENDPOINTS.contains(&"next"));
        assert!(ENDPOINTS.contains(&"feedback"));
        assert!(ENDPOINTS.contains(&"music/get_search_suggestions"));
        assert!(ENDPOINTS.contains(&"music/get_queue"));
        assert!(ENDPOINTS.contains(&"get_transcript"));
        assert!(ENDPOINTS.contains(&"visitor_id"));
        assert!(ENDPOINTS.contains(&"account/account_menu"));
        assert!(ENDPOINTS.contains(&"like/like"));
        assert!(ENDPOINTS.contains(&"like/removelike"));
        assert!(ENDPOINTS.contains(&"subscription/subscribe+unsubscribe"));
        assert!(ENDPOINTS.contains(&"browse/edit_playlist"));
        assert!(ENDPOINTS.contains(&"playlist/create"));
        assert!(ENDPOINTS.contains(&"playlist/delete"));
        assert!(ENDPOINTS.contains(&"playlist_image_upload"));
        assert!(ENDPOINTS.contains(&"timedtext"));
        assert!(ENDPOINTS.contains(&"sw.js_data"));
        assert!(ENDPOINTS.contains(&"videostats_play_reporting"));
        assert!(ENDPOINTS.contains(&"returnyoutubedislike_votes"));
        assert!(ENDPOINTS.contains(&"botguard_create_generate_it"));
    }

    #[test]
    fn transcript_params_match_the_kotlin_envelope() {
        // "\n" + Char(11) + videoId, base64 (InnerTube.kt:418-420)
        assert_eq!(transcript_params("dQw4w9WgXcQ"), "CgtkUXc0dzlXZ1hjUQ==");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(transcript_params("abc"))
            .unwrap();
        assert_eq!(decoded, b"\n\x0babc");
    }

    #[test]
    fn music_stats_url_replaces_only_s_youtube() {
        // YouTube.kt:1300
        assert_eq!(
            music_stats_url("https://s.youtube.com/api/stats/watchback?x=1"),
            "https://music.youtube.com/api/stats/watchback?x=1"
        );
        assert_eq!(
            music_stats_url("https://music.youtube.com/api/other"),
            "https://music.youtube.com/api/other"
        );
    }

    #[test]
    fn playback_nonce_shape() {
        // YouTube.kt:1266-1268: 16 chars from the 64-char alphabet
        const ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
        for _ in 0..64 {
            let nonce = new_playback_nonce();
            assert_eq!(nonce.len(), 16);
            assert!(nonce.chars().all(|c| ALPHABET.contains(c)));
        }
    }
}
