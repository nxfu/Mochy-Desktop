//! Watcher/invalidation tests: writes through the Db actor fire the
//! table-keyed broadcast after commit; the std-mpsc watcher shape filters
//! by table set; aborted transactions fire nothing.

use std::sync::Arc;

use mochy_db::Db;
use mochy_db::watch::{Table, TableSet};

async fn fresh_db() -> Arc<Db> {
    Arc::new(Db::open_in_memory().await.expect("open in-memory db"))
}

#[tokio::test]
async fn write_fires_broadcast_invalidation() {
    let db = fresh_db().await;
    let mut handle = db.subscribe();

    let inserted = db
        .write(TableSet::of(&[Table::Song]), move |conn| {
            mochy_db::queries::insert_song(
                conn,
                &mochy_db::SongEntity {
                    id: "song1".into(),
                    title: "Song One".into(),
                    ..Default::default()
                },
            )
        })
        .await
        .unwrap();
    assert!(inserted);

    let event = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handle.recv(TableSet::of(&[Table::Song])),
    )
    .await
    .expect("invalidation within timeout")
    .expect("db alive");
    assert!(event.contains(TableSet::of(&[Table::Song])));
    assert_eq!(event.table_names(), vec!["song"]);
}

#[tokio::test]
async fn std_watch_filters_by_table_set() {
    let db = fresh_db().await;
    let (rx, _guard) = db.watch(TableSet::of(&[Table::Song, Table::Event]));

    // A lyrics write must NOT reach the song/event watcher.
    db.write(TableSet::of(&[Table::Lyrics]), |conn| {
        mochy_db::queries::upsert_lyrics(
            conn,
            &mochy_db::LyricsEntity {
                id: "song1".into(),
                lyrics: "la la la".into(),
                ..Default::default()
            },
        )
    })
    .await
    .unwrap();

    // A song write must, carrying only the intersection.
    db.write(TableSet::of(&[Table::Song]), |conn| {
        mochy_db::queries::insert_song(
            conn,
            &mochy_db::SongEntity {
                id: "song1".into(),
                title: "Song One".into(),
                ..Default::default()
            },
        )
    })
    .await
    .unwrap();

    let event = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("song invalidation");
    assert_eq!(event.table_names(), vec!["song"]);

    // Nothing else queued (the lyrics write was filtered out).
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(100))
            .is_err()
    );
}

#[tokio::test]
async fn aborted_transaction_fires_nothing() {
    let db = fresh_db().await;
    let (rx, _guard) = db.watch(TableSet::of(&[Table::Song]));

    // FK violation inside the transaction: song_artist_map -> song.
    let result = db
        .write(TableSet::of(&[Table::SongArtistMap]), |conn| {
            conn.execute(
                "INSERT INTO song_artist_map (songId, artistId, position) VALUES ('ghost', 'ghost2', 0)",
                [],
            )?;
            Ok(())
        })
        .await;
    assert!(result.is_err(), "FK violation must abort the write");

    // No invalidation, and the writer is usable afterwards.
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(100))
            .is_err()
    );
    db.write(TableSet::of(&[Table::Song]), |conn| {
        mochy_db::queries::insert_song(
            conn,
            &mochy_db::SongEntity {
                id: "ok".into(),
                title: "Still works".into(),
                ..Default::default()
            },
        )
    })
    .await
    .unwrap();
    assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok());
}

#[tokio::test]
async fn watch_guard_drops_subscription() {
    let db = fresh_db().await;
    let (rx, guard) = db.watch(TableSet::of(&[Table::Song]));
    drop(guard);

    db.write(TableSet::of(&[Table::Song]), |conn| {
        mochy_db::queries::insert_song(
            conn,
            &mochy_db::SongEntity {
                id: "song1".into(),
                title: "Song One".into(),
                ..Default::default()
            },
        )
    })
    .await
    .unwrap();

    // The receiver is disconnected once the guard drops.
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(100))
            .is_err()
    );
}

#[tokio::test]
async fn reads_run_on_the_pool_while_the_writer_is_busy() {
    let db = fresh_db().await;
    db.write(TableSet::of(&[Table::Song]), |conn| {
        mochy_db::queries::insert_song(
            conn,
            &mochy_db::SongEntity {
                id: "song1".into(),
                title: "Song One".into(),
                ..Default::default()
            },
        )
    })
    .await
    .unwrap();

    // A read from the current thread while the actor holds no lock.
    let count = db
        .read(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM song", [], |r| r.get::<_, i64>(0))?))
        .unwrap();
    assert_eq!(count, 1);
}
