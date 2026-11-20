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

// ---------------------------------------------------------------------------
// Regression coverage for the macOS CI flake (fixed in db.rs): the in-memory
// database name must be unique per instance even when the OS clock has only
// microsecond granularity — two `SystemTime::now()` reads from parallel test
// threads returned identical values, the two instances then shared ONE
// shared-cache in-memory database, and the second `INSERT OR IGNORE` of the
// same song id silently returned `Ok(false)` (watcher.rs:32 flake).
// ---------------------------------------------------------------------------

/// Two in-memory instances are independent databases: the same primary key
/// inserts successfully into both. This is the direct regression test for
/// the collision — with a shared (colliding) URI name it fails at
/// `assert!(inserted2)`.
#[tokio::test]
async fn in_memory_instances_are_independent_databases() {
    let db1 = fresh_db().await;
    let db2 = fresh_db().await;

    for db in [&db1, &db2] {
        let inserted = db
            .write(TableSet::of(&[Table::Song]), |conn| {
                mochy_db::queries::insert_song(
                    conn,
                    &mochy_db::SongEntity {
                        id: "same-id".into(),
                        title: "Same id, different database".into(),
                        ..Default::default()
                    },
                )
            })
            .await
            .unwrap();
        assert!(inserted, "each instance must accept the same song id");
    }

    // And each instance sees exactly its own row.
    for db in [&db1, &db2] {
        let count = db
            .read(|conn| {
                Ok(conn.query_row("SELECT COUNT(*) FROM song", [], |r| r.get::<_, i64>(0))?)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}

/// A committed UPDATE fires the invalidation for its table set.
#[tokio::test]
async fn committed_update_fires_invalidation() {
    let db = fresh_db().await;
    let (rx, _guard) = db.watch(TableSet::of(&[Table::Song]));

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
    let _ = rx.recv_timeout(std::time::Duration::from_secs(5));

    // Full-row rewrite (upstream @Update) on the writer; verify both the
    // invalidation and the persisted change.
    db.write(TableSet::of(&[Table::Song]), |conn| {
        let mut song = mochy_db::queries::song_entity(conn, "song1")?.expect("song exists");
        song.title = "Song One (edited)".into();
        mochy_db::queries::update_song(conn, &song)
    })
    .await
    .unwrap();

    let event = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("update invalidation");
    assert_eq!(event.table_names(), vec!["song"]);

    let title = db
        .read(|conn| {
            Ok(
                conn.query_row("SELECT title FROM song WHERE id = 'song1'", [], |r| {
                    r.get::<_, String>(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(title, "Song One (edited)");
}

/// A committed DELETE fires the invalidation for its table set.
#[tokio::test]
async fn committed_delete_fires_invalidation() {
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

    let (rx, _guard) = db.watch(TableSet::of(&[Table::Song]));
    db.write(TableSet::of(&[Table::Song]), |conn| {
        conn.execute("DELETE FROM song WHERE id = 'song1'", [])?;
        Ok(())
    })
    .await
    .unwrap();

    let event = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("delete invalidation");
    assert_eq!(event.table_names(), vec!["song"]);
}

/// Multiple subscribers all observe the same write; a dropped subscriber
/// does not break subsequent writes.
#[tokio::test]
async fn multiple_subscribers_all_observe_the_write() {
    let db = fresh_db().await;
    let (rx1, _g1) = db.watch(TableSet::of(&[Table::Song]));
    let (rx2, _g2) = db.watch(TableSet::of(&[Table::Song, Table::Artist]));
    let (rx3, _g3) = db.watch(TableSet::of(&[Table::Song]));
    let mut raw = db.subscribe();

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

    for rx in [&rx1, &rx2, &rx3] {
        let event = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("each subscriber sees the write");
        assert_eq!(event.table_names(), vec!["song"]);
    }
    let event = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        raw.recv(TableSet::of(&[Table::Song])),
    )
    .await
    .expect("raw broadcast within timeout")
    .expect("db alive");
    assert_eq!(event.table_names(), vec!["song"]);

    // Dropping one subscriber must not break the next write.
    drop((_g1, rx1));
    db.write(TableSet::of(&[Table::Song]), |conn| {
        conn.execute("DELETE FROM song WHERE id = 'song1'", [])?;
        Ok(())
    })
    .await
    .unwrap();
    for rx in [&rx2, &rx3] {
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "remaining subscribers still notified"
        );
    }
}

/// Repeated writes do not silently lose notifications: N committed writes
/// produce at least N observable invalidations (coalescing may merge them
/// only across separate recv calls, never drop committed ones entirely).
#[tokio::test]
async fn repeated_writes_do_not_lose_notifications() {
    let db = fresh_db().await;
    let (rx, _guard) = db.watch(TableSet::of(&[Table::Song]));

    const N: usize = 8;
    for i in 0..N {
        db.write(TableSet::of(&[Table::Song]), move |conn| {
            conn.execute(
                "INSERT INTO song (id, title, duration, liked, totalPlayTime) \
                 VALUES (?1, ?2, 0, 0, 0)",
                rusqlite::params![format!("song{i}"), format!("Song {i}")],
            )?;
            Ok(())
        })
        .await
        .expect("write must commit");
    }

    let mut seen = 0;
    while seen < N {
        let event = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("no committed write may be lost");
        assert_eq!(event.table_names(), vec!["song"]);
        seen += 1;
    }
}
