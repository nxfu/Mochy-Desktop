//! Repository tests — at least one behavior check per repository, driven
//! through the async actor (writes) and the reader pool (reads).

use std::sync::Arc;

use mochy_db::queries::{AlbumSort, ArtistSort, PlaylistSort, SongSort};
use mochy_db::{
    AlbumEntity, Db, FormatEntity, ImportedSong, LyricsEntity, MAX_SPEED_DIAL_ITEMS,
    PlaylistEntity, RecognitionHistory, Repositories, SetVideoIdEntity, SongEdit, SongEntity,
    SpeedDialItem, Table, TableSet,
};

const T0: i64 = 1_795_132_800_000; // 2026-11-20T00:00:00Z

async fn db_with_repos() -> (Arc<Db>, Repositories) {
    let db = Arc::new(Db::open_in_memory().await.expect("open db"));
    let repos = Repositories::new(Arc::clone(&db));
    (db, repos)
}

async fn seed_song(repos: &Repositories, id: &str, title: &str, artist: Option<(&str, &str)>) {
    let artists = artist
        .map(|(aid, name)| vec![(Some(aid.to_string()), name.to_string())])
        .unwrap_or_default();
    repos
        .songs
        .insert_song_with_artists(
            SongEntity {
                id: id.into(),
                title: title.into(),
                duration: 180,
                ..Default::default()
            },
            artists,
            T0,
        )
        .await
        .unwrap();
}

// --- SongRepository -------------------------------------------------------

#[tokio::test]
async fn song_repo_inserts_reads_and_toggles_like() {
    let (_db, repos) = db_with_repos().await;
    seed_song(&repos, "s1", "Nightcall", Some(("UCA1", "Kavinsky"))).await;

    // Relation read: artists come back position-ordered.
    let song = repos.songs.song("s1").unwrap();
    let song = song.expect("song exists");
    assert_eq!(song.song.title, "Nightcall");
    assert_eq!(song.artists.len(), 1);
    assert_eq!(song.artists[0].name, "Kavinsky");

    // toggleLike state machine (SongEntity.kt:64-72) — the persistence
    // half only; the YouTube call is Phase 14's.
    let updated = repos
        .songs
        .toggle_like("s1".into(), T0 + 1000)
        .await
        .unwrap()
        .expect("song");
    assert!(updated.liked);
    assert_eq!(updated.liked_date, Some(T0 + 1000));
    // A like backfills inLibrary and sets addedByLike (no library date yet).
    assert_eq!(updated.in_library, Some(T0 + 1000));
    assert!(updated.added_by_like);

    // Unliking clears likedDate but keeps the library date.
    let updated = repos
        .songs
        .toggle_like("s1".into(), T0 + 2000)
        .await
        .unwrap()
        .unwrap();
    assert!(!updated.liked);
    assert_eq!(updated.liked_date, None);
    assert_eq!(updated.in_library, Some(T0 + 1000));
    assert!(updated.added_by_like);

    // Liked list + count.
    repos
        .songs
        .toggle_like("s1".into(), T0 + 3000)
        .await
        .unwrap();
    assert_eq!(repos.songs.liked_songs_count().unwrap(), 1);
    let liked = repos
        .songs
        .liked_songs(SongSort::CreateDate, false)
        .unwrap();
    assert_eq!(liked.len(), 1);
    assert_eq!(liked[0].id(), "s1");

    // likedSongIdsSince (DatabaseDao.kt:1211-1213).
    let since = repos.songs.liked_song_ids_since(T0 + 2500).unwrap();
    assert_eq!(since, vec!["s1".to_string()]);
}

// --- ArtistRepository -----------------------------------------------------

#[tokio::test]
async fn artist_repo_lists_and_bookmarks() {
    let (_db, repos) = db_with_repos().await;
    seed_song(&repos, "s1", "Nightcall", Some(("UCA1", "Kavinsky"))).await;
    // In library so the artist shows in artists().
    repos
        .songs
        .set_in_library("s1".into(), Some(T0))
        .await
        .unwrap();

    let artists = repos.artists.artists(ArtistSort::Name, false).unwrap();
    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].artist.name, "Kavinsky");
    assert_eq!(artists[0].song_count, 1);

    // Bookmark toggle (ArtistEntity.localToggleLike).
    let artist = repos
        .artists
        .toggle_bookmark("UCA1".into(), T0 + 100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(artist.bookmarked_at, Some(T0 + 100));
    let bookmarked = repos
        .artists
        .artists_bookmarked(ArtistSort::CreateDate, false)
        .unwrap();
    assert_eq!(bookmarked.len(), 1);

    // Artist songs come through the map.
    let songs = repos
        .artists
        .artist_songs("UCA1", SongSort::Name, false)
        .unwrap();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].id(), "s1");
}

// --- AlbumRepository ------------------------------------------------------

#[tokio::test]
async fn album_repo_orders_songs_by_index() {
    let (_db, repos) = db_with_repos().await;
    // Album + three songs with disc indexes 2, 0, 1.
    repos
        .albums
        .insert_album(AlbumEntity {
            id: "AL1".into(),
            title: "OutRun".into(),
            song_count: 3,
            duration: 540,
            ..Default::default()
        })
        .await
        .unwrap();
    for (sid, title) in [("s1", "Beta"), ("s2", "Alpha"), ("s3", "Gamma")] {
        seed_song(&repos, sid, title, None).await;
        repos
            .songs
            .set_in_library(sid.into(), Some(T0))
            .await
            .unwrap();
    }
    repos
        .db()
        .write(TableSet::of(&[Table::SongAlbumMap, Table::Song]), |conn| {
            for (sid, index) in [("s1", 2), ("s2", 0), ("s3", 1)] {
                conn.execute(
                    "INSERT OR IGNORE INTO song_album_map (songId, albumId, `index`) VALUES (?1, 'AL1', ?2)",
                    rusqlite::params![sid, index],
                )?;
                // The library-albums query keys on song.albumId
                // (DatabaseDao.kt:870), like setSongAlbum does.
                conn.execute(
                    "UPDATE song SET albumId = 'AL1', albumName = 'OutRun' WHERE id = ?1",
                    rusqlite::params![sid],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();

    // The sorted_song_album_map consuming query: ORDER BY `index`, rowid.
    let songs = repos.albums.album_songs("AL1").unwrap();
    let titles: Vec<&str> = songs.iter().map(|s| s.title()).collect();
    assert_eq!(titles, vec!["Alpha", "Gamma", "Beta"]);

    let albums = repos.albums.albums(AlbumSort::Name, false).unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].album.title, "OutRun");
}

// --- PlaylistRepository ---------------------------------------------------

#[tokio::test]
async fn playlist_repo_preview_adds_and_moves() {
    let (_db, repos) = db_with_repos().await;
    repos
        .playlists
        .insert_playlist(PlaylistEntity {
            id: "LP1".into(),
            name: "Mix".into(),
            bookmarked_at: Some(T0),
            is_editable: true,
            ..Default::default()
        })
        .await
        .unwrap();
    for (sid, title) in [
        ("s1", "One"),
        ("s2", "Two"),
        ("s3", "Three"),
        ("s4", "Four"),
    ] {
        seed_song(&repos, sid, title, None).await;
    }

    repos
        .playlists
        .add_songs(
            "LP1".into(),
            vec!["s1".into(), "s2".into(), "s3".into(), "s4".into()],
        )
        .await
        .unwrap();

    // Preview thumbnails: the upstream view is `position <= 3` verbatim —
    // with 0-based positions that is the first FOUR entries (PlaylistSongMapPreview.kt).
    let playlist = repos.playlists.playlist("LP1").unwrap().expect("playlist");
    assert_eq!(playlist.song_count, 4);
    assert_eq!(playlist.song_thumbnails.len(), 4);

    // Full listing is position-ordered.
    let songs = repos.playlists.playlist_songs("LP1").unwrap();
    let ids: Vec<&str> = songs.iter().map(|ps| ps.song.id()).collect();
    assert_eq!(ids, vec!["s1", "s2", "s3", "s4"]);

    // move(1 -> 3) (DatabaseDao.kt:1494-1510).
    repos.playlists.move_song("LP1".into(), 1, 3).await.unwrap();
    let songs = repos.playlists.playlist_songs("LP1").unwrap();
    let ids: Vec<&str> = songs.iter().map(|ps| ps.song.id()).collect();
    assert_eq!(ids, vec!["s1", "s3", "s4", "s2"]);

    // Pinned playlists list.
    let lists = repos
        .playlists
        .playlists(PlaylistSort::Name, false)
        .unwrap();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].playlist.name, "Mix");
}

// --- FormatRepository -----------------------------------------------------

#[tokio::test]
async fn format_repo_round_trip() {
    let (_db, repos) = db_with_repos().await;
    repos
        .formats
        .upsert(FormatEntity {
            id: "s1".into(),
            itag: 251,
            mime_type: "audio/webm".into(),
            codecs: "opus".into(),
            bitrate: 160_000,
            sample_rate: Some(48_000),
            content_length: 4_000_000,
            loudness_db: Some(-3.5),
            perceptual_loudness_db: Some(-2.1),
            playback_url: None,
        })
        .await
        .unwrap();
    let f = repos.formats.format("s1").unwrap().expect("format");
    assert_eq!(f.itag, 251);
    assert_eq!(f.perceptual_loudness_db, Some(-2.1));

    repos.formats.delete("s1".into()).await.unwrap();
    assert!(repos.formats.format("s1").unwrap().is_none());
}

// --- LyricsRepository -----------------------------------------------------

#[tokio::test]
async fn lyrics_repo_sentinel_and_clear() {
    let (_db, repos) = db_with_repos().await;
    // Negative cache: LYRICS_NOT_FOUND (LyricsEntity.kt companion).
    repos
        .lyrics
        .upsert(LyricsEntity {
            id: "s1".into(),
            lyrics: LyricsEntity::LYRICS_NOT_FOUND.into(),
            provider: "LRCLIB".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let l = repos.lyrics.lyrics("s1").unwrap().expect("sentinel row");
    assert_eq!(l.lyrics, LyricsEntity::LYRICS_NOT_FOUND);

    repos
        .lyrics
        .upsert(LyricsEntity {
            id: "s2".into(),
            lyrics: "[00:01.00] line".into(),
            provider: "USER_EDITED".into(),
            ..Default::default()
        })
        .await
        .unwrap();

    // clearFetchedLyrics keeps USER_EDITED (DatabaseDao.kt:1588-1590).
    let removed = repos
        .lyrics
        .clear_fetched(vec!["USER_EDITED".into()])
        .await
        .unwrap();
    assert_eq!(removed, 1);
    assert!(repos.lyrics.lyrics("s1").unwrap().is_none());
    assert!(repos.lyrics.lyrics("s2").unwrap().is_some());
}

// --- EventRepository ------------------------------------------------------

#[tokio::test]
async fn event_repo_report_play_writes_three_tables() {
    let (_db, repos) = db_with_repos().await;
    seed_song(&repos, "s1", "Nightcall", None).await;

    repos
        .events
        .report_play("s1".into(), T0, 45_000)
        .await
        .unwrap();
    repos
        .events
        .report_play("s1".into(), T0 + 60_000, 30_000)
        .await
        .unwrap();

    // event rows + playCount bucket + totalPlayTime.
    assert_eq!(repos.events.total_listening_time_ms().unwrap(), 75_000);
    assert_eq!(repos.events.lifetime_play_count("s1").unwrap(), 2);
    let events = repos.events.events_in_range(T0 - 1, T0 + 120_000).unwrap();
    assert_eq!(events.len(), 2);
    let song = repos.songs.song("s1").unwrap().unwrap();
    assert_eq!(song.song.total_play_time, 75_000);

    // Playlist events land too (the playlist row must exist — FK cascade).
    repos
        .playlists
        .insert_playlist(PlaylistEntity {
            id: "LP1".into(),
            name: "Mix".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    repos
        .events
        .report_playlist_play("LP1".into(), T0)
        .await
        .unwrap();
}

// --- SearchHistoryRepository ----------------------------------------------

#[tokio::test]
async fn search_history_upsert_moves_to_top() {
    let (_db, repos) = db_with_repos().await;
    repos
        .search_history
        .upsert("kavinsky".into())
        .await
        .unwrap();
    repos
        .search_history
        .upsert("nightcall".into())
        .await
        .unwrap();
    repos
        .search_history
        .upsert("kavinsky".into())
        .await
        .unwrap();

    let all = repos.search_history.search("").unwrap();
    assert_eq!(all.len(), 2);
    // Re-searching a term moves it to the top (REPLACE on unique query).
    assert_eq!(all[0].query, "kavinsky");

    repos
        .search_history
        .delete("nightcall".into())
        .await
        .unwrap();
    assert_eq!(repos.search_history.search("").unwrap().len(), 1);
}

// --- RadioRepository ------------------------------------------------------

#[tokio::test]
async fn radio_repo_related_songs_dedup() {
    let (_db, repos) = db_with_repos().await;
    seed_song(&repos, "s1", "Seed", None).await;
    seed_song(&repos, "s2", "Neighbour", None).await;

    // Duplicate adjacency rows collapse (GROUP BY relatedSongId).
    repos
        .radio
        .insert_related_song("s1".into(), "s2".into())
        .await
        .unwrap();
    repos
        .radio
        .insert_related_song("s1".into(), "s2".into())
        .await
        .unwrap();
    assert!(repos.radio.has_related_songs("s1").unwrap());
    let related = repos.radio.related_songs("s1").unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id(), "s2");

    // set_video_id cache round-trip.
    repos
        .radio
        .upsert_set_video_id(SetVideoIdEntity {
            video_id: "s2".into(),
            set_video_id: Some("ABCD".into()),
        })
        .await
        .unwrap();
    let sv = repos.radio.set_video_id("s2").unwrap().unwrap();
    assert_eq!(sv.set_video_id.as_deref(), Some("ABCD"));
}

// --- RecognitionRepository ------------------------------------------------

#[tokio::test]
async fn recognition_repo_search_like_and_clear() {
    let (_db, repos) = db_with_repos().await;
    repos
        .recognition
        .upsert(RecognitionHistory {
            track_id: "t1".into(),
            title: "Midnight City".into(),
            artist: "M83".into(),
            recognized_at: T0,
            ..Default::default()
        })
        .await
        .unwrap();
    let hits = repos.recognition.search("midnight").unwrap();
    assert_eq!(hits.len(), 1);
    repos.recognition.set_liked(hits[0].id, true).await.unwrap();
    let hits = repos.recognition.search("M83").unwrap();
    assert!(hits[0].liked);
    repos.recognition.clear().await.unwrap();
    assert!(repos.recognition.search("").unwrap().is_empty());
}

// --- SpeedDialRepository --------------------------------------------------

#[tokio::test]
async fn speed_dial_enforces_the_nine_item_cap() {
    let (_db, repos) = db_with_repos().await;
    for i in 0..12 {
        repos
            .speed_dial
            .pin(SpeedDialItem {
                id: format!("s{i}"),
                title: format!("Item {i}"),
                item_type: "SONG".into(),
                create_date: T0 + i as i64 * 1000,
                ..Default::default()
            })
            .await
            .unwrap();
    }
    let items = repos.speed_dial.items().unwrap();
    assert_eq!(items.len(), MAX_SPEED_DIAL_ITEMS, "3x3 grid cap");
    // The oldest (s0, s1, s2) were evicted; newest first.
    assert_eq!(items[0].id, "s11");
    assert!(!items.iter().any(|i| i.id == "s0"));
    assert!(repos.speed_dial.is_pinned("s11").unwrap());
}

// --- SongEditRepository ---------------------------------------------------

#[tokio::test]
async fn song_edit_overlay_round_trip() {
    let (_db, repos) = db_with_repos().await;
    seed_song(&repos, "s1", "Wrong Title", None).await;
    repos
        .song_edits
        .upsert(SongEdit {
            song_id: "s1".into(),
            title: Some("Right Title".into()),
            artists: Some(r#"[{"id":"UCA1","name":"Fixed Artist"}]"#.into()),
            album: Some("Fixed Album".into()),
            year: Some(2016),
            cover: None,
            video_id: None,
            no_video: false,
        })
        .await
        .unwrap();
    let edit = repos.song_edits.song_edit("s1").unwrap().expect("edit");
    assert_eq!(edit.title.as_deref(), Some("Right Title"));
    assert!(edit.artists.as_deref().unwrap().contains("Fixed Artist"));
    assert_eq!(repos.song_edits.song_edits().unwrap().len(), 1);

    repos.song_edits.delete("s1".into()).await.unwrap();
    assert!(repos.song_edits.song_edit("s1").unwrap().is_none());
}

// --- ImportBookkeepingRepository ------------------------------------------

#[tokio::test]
async fn import_bookkeeping_counts_sessions_and_undo() {
    let (_db, repos) = db_with_repos().await;
    seed_song(&repos, "s1", "Imported Song", None).await;
    repos
        .songs
        .set_in_library("s1".into(), Some(T0))
        .await
        .unwrap();

    let session = repos
        .imports
        .start_session("spotify".into(), T0)
        .await
        .unwrap();
    repos
        .imports
        .insert_imported_song(ImportedSong {
            song_id: "s1".into(),
            source: "spotify".into(),
            imported_at: T0,
        })
        .await
        .unwrap();

    let counts = repos.imports.source_counts().unwrap();
    assert_eq!(counts, vec![("spotify".to_string(), 1)]);

    let sessions = repos.imports.sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].source, "spotify");

    // Undo the session.
    repos.imports.delete_session(session).await.unwrap();
    assert!(repos.imports.sessions().unwrap().is_empty());
}

// --- A repo-level download-state test -------------------------------------

#[tokio::test]
async fn song_repo_update_downloaded_info() {
    let (_db, repos) = db_with_repos().await;
    seed_song(&repos, "s1", "Offline Song", None).await;
    repos
        .songs
        .update_downloaded_info("s1".into(), true, Some(T0 + 5000))
        .await
        .unwrap();
    let downloaded = repos
        .songs
        .downloaded_songs(SongSort::CreateDate, false)
        .unwrap();
    assert_eq!(downloaded.len(), 1);
    assert_eq!(downloaded[0].song.date_download, Some(T0 + 5000));
}
