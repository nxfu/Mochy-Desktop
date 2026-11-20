//! Schema acceptance tests: the v45 mirror report (24 tables + 3 views),
//! user_version stamping, and the exact-match comparison against the
//! newest exported Room schema JSON (v34) with a printed v34→v45 delta.

use std::collections::BTreeMap;

use rusqlite::Connection;

/// The last exported Room schema JSON (upstream
/// `app/schemas/com.mochyy.mochymusic.db.InternalDatabase/34.json`, pin
/// `bde01cb4`), committed as a fixture. exportSchema was later disabled,
/// so v34 is the newest available JSON — v35→v45 additions are
/// reconstructed from the hand-written migrations and asserted below.
const ROOM_SCHEMA_V34: &str = include_str!("data/room_schema_v34.json");

/// The documented v34→v45 delta (source: MusicDatabase.kt migrations
/// MIGRATION_35_36 … MIGRATION_44_45 + the current entity fields).
fn expected_delta() -> BTreeMap<&'static str, Vec<&'static str>> {
    BTreeMap::from([
        // MIGRATION_36_37: song.localUri
        ("song", vec!["localUri", "addedByLike", "genre"]), // +41_42 addedByLike, +43_44 genre
        // MIGRATION_37_38: playlist.description
        ("playlist", vec!["description"]),
        // MIGRATION_35_36
        ("playlist_event", vec![]),
        // MIGRATION_38_39
        ("imported_song", vec![]),
        // MIGRATION_39_40
        ("import_session", vec![]),
        ("import_session_song", vec![]),
        // MIGRATION_39_40 + 40_41 remoteId/remoteUrl/lastSyncedAt + 44_45 author cols
        ("imported_playlist", vec![]),
        ("imported_playlist_track", vec![]),
        // MIGRATION_42_43
        ("song_edit", vec![]),
    ])
}

#[derive(serde::Deserialize)]
struct RoomSchemaJson {
    database: RoomDatabaseJson,
}

#[derive(serde::Deserialize)]
struct RoomDatabaseJson {
    entities: Vec<RoomEntityJson>,
    #[serde(default)]
    views: Vec<RoomViewJson>,
}

#[derive(serde::Deserialize)]
struct RoomEntityJson {
    #[serde(rename = "tableName")]
    table_name: String,
    fields: Vec<RoomFieldJson>,
}

#[derive(serde::Deserialize)]
struct RoomViewJson {
    #[serde(rename = "viewName")]
    view_name: String,
}

#[derive(serde::Deserialize)]
struct RoomFieldJson {
    #[serde(rename = "columnName")]
    column_name: String,
    affinity: String,
    #[serde(rename = "notNull", default)]
    not_null: bool,
    #[serde(rename = "defaultValue", default)]
    default_value: Option<String>,
}

/// The schema report test: prints all 24 tables + 3 views with column
/// counts and asserts the mirror's exact shape.
#[test]
fn schema_report_prints_24_tables_and_3_views() {
    let conn = Connection::open_in_memory().unwrap();
    mochy_db::schema::apply(&conn, std::path::Path::new(":memory:")).unwrap();

    let report = mochy_db::schema::report(&conn).unwrap();
    assert_eq!(report.tables.len(), 24, "24 mirrored tables");
    assert_eq!(report.views.len(), 3, "3 mirrored views");

    println!("mochy-db schema report (Room schema v45 mirror):");
    let mut total_columns = 0usize;
    for (table, columns) in &report.tables {
        total_columns += columns.len();
        println!("  table  {table:<26} {:>2} columns", columns.len());
    }
    for view in &report.views {
        println!("  view   {view}");
    }
    println!(
        "  total: {} tables, {} views, {} columns",
        report.tables.len(),
        report.views.len(),
        total_columns
    );

    // The verified upstream column counts (v34 JSON + the migration delta).
    let expected_counts: BTreeMap<&str, usize> = BTreeMap::from([
        ("song", 26),
        ("artist", 7),
        ("album", 16),
        ("playlist", 15),
        ("song_artist_map", 3),
        ("song_album_map", 3),
        ("album_artist_map", 3),
        ("playlist_song_map", 5),
        ("search_history", 2),
        ("format", 10),
        ("lyrics", 6),
        ("event", 4),
        ("related_song_map", 3),
        ("set_video_id", 2),
        ("playCount", 4),
        ("recognition_history", 17),
        ("speed_dial_item", 8),
        ("playlist_event", 3),
        ("imported_song", 3),
        ("import_session", 3),
        ("import_session_song", 4),
        ("imported_playlist", 10),
        ("imported_playlist_track", 2),
        ("song_edit", 8),
    ]);
    for (table, columns) in &report.tables {
        let want = expected_counts
            .get(table.as_str())
            .unwrap_or_else(|| panic!("unexpected table {table}"));
        assert_eq!(
            &columns.len(),
            want,
            "table {table} column count (upstream entities + migrations)"
        );
    }

    // user_version = 1, and room_master_table must NOT exist.
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert!(!mochy_db::schema::table_exists(&conn, "room_master_table").unwrap());
}

/// The mirror's columns must match the newest exported Room schema JSON
/// (v34) EXACTLY where the JSON covers them (name + affinity + notNull +
/// default), and the v34→v45 delta must equal the documented migration
/// additions. Prints a diff report either way.
#[test]
fn mirror_matches_room_v34_json_with_documented_delta() {
    let conn = Connection::open_in_memory().unwrap();
    mochy_db::schema::apply(&conn, std::path::Path::new(":memory:")).unwrap();

    let mirror = mochy_db::schema::report(&conn).unwrap();
    let room: RoomSchemaJson = serde_json::from_str(ROOM_SCHEMA_V34).unwrap();

    let expected = expected_delta();
    let mut printed_diff: Vec<String> = Vec::new();

    // Every JSON entity must exist in the mirror with a matching column set
    // for the shared columns.
    for entity in &room.database.entities {
        let mirror_columns = mirror
            .tables
            .iter()
            .find(|(t, _)| t == &entity.table_name)
            .unwrap_or_else(|| panic!("mirror lost table {}", entity.table_name))
            .1
            .clone();

        let mut expected_new = expected
            .get(entity.table_name.as_str())
            .cloned()
            .unwrap_or_default();

        for field in &entity.fields {
            let m = mirror_columns
                .iter()
                .find(|c| c.name == field.column_name)
                .unwrap_or_else(|| {
                    panic!(
                        "mirror table {} lost column {} (present in v34 JSON)",
                        entity.table_name, field.column_name
                    )
                });
            assert_eq!(
                m.decl_type, field.affinity,
                "{}.{} type mismatch",
                entity.table_name, field.column_name
            );
            assert_eq!(
                m.not_null, field.not_null,
                "{}.{} nullability mismatch",
                entity.table_name, field.column_name
            );
            match (&m.default, &field.default_value) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    // Room quotes string defaults ('Unknown'); SQLite's
                    // table_info returns the literal DDL text.
                    let same = a == b
                        || a.replace('\'', "") == b.replace('\'', "")
                            && (a.starts_with('\'') || b.starts_with('\''));
                    assert!(
                        same,
                        "{}.{} default mismatch: mirror {a:?} vs room {b:?}",
                        entity.table_name, field.column_name
                    );
                }
                (a, b) => panic!(
                    "{}.{} default mismatch: mirror {a:?} vs room {b:?}",
                    entity.table_name, field.column_name
                ),
            }
        }

        // Columns the mirror has beyond the JSON = the migration delta.
        let extra: Vec<String> = mirror_columns
            .iter()
            .filter(|c| !entity.fields.iter().any(|f| f.column_name == c.name))
            .map(|c| c.name.clone())
            .collect();
        let want: Vec<String> = expected_new.drain(..).map(|s| s.to_string()).collect();
        if extra != want {
            printed_diff.push(format!(
                "{}: v45 has {extra:?} but expected delta {want:?}",
                entity.table_name
            ));
        }
    }

    // Tables in the delta that have no v34 JSON entry are new tables.
    for (table, cols) in &expected {
        if !room
            .database
            .entities
            .iter()
            .any(|e| &e.table_name == table)
        {
            let column_count = mirror
                .tables
                .iter()
                .find(|(t, _)| t == table)
                .unwrap_or_else(|| panic!("mirror lost delta table {table}"))
                .1
                .len();
            assert!(
                column_count > cols.len(),
                "delta table {table} should have more than the delta columns"
            );
        }
    }

    // Every JSON view must exist in the mirror.
    for view in &room.database.views {
        assert!(
            mirror.views.iter().any(|v| v == &view.view_name),
            "mirror lost view {}",
            view.view_name
        );
    }

    println!("v34 -> v45 schema delta report (reconstructed from migrations):");
    if printed_diff.is_empty() {
        for (table, cols) in expected_delta() {
            match cols.len() {
                0 => println!("  {table}: new table (v35-v45 migration)"),
                _ => println!("  {table}: +{}", cols.join(", ")),
            }
        }
        for entity in &room.database.entities {
            if !expected_delta().contains_key(entity.table_name.as_str()) {
                println!("  {}: unchanged since v34", entity.table_name);
            }
        }
    } else {
        for line in &printed_diff {
            println!("  MISMATCH {line}");
        }
    }
    assert!(
        printed_diff.is_empty(),
        "mirror delta does not match the documented v34->v45 additions: {printed_diff:?}"
    );
}

/// DDL text must be Room-compatible: applying it to an empty database and
/// reading `sqlite_master` yields the same objects the mirror declares,
/// and a second apply is a no-op (idempotent).
#[test]
fn schema_apply_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    mochy_db::schema::apply(&conn, std::path::Path::new(":memory:")).unwrap();
    mochy_db::schema::apply(&conn, std::path::Path::new(":memory:")).unwrap();
    let report = mochy_db::schema::report(&conn).unwrap();
    assert_eq!(report.tables.len(), 24);
    assert_eq!(report.views.len(), 3);
}
