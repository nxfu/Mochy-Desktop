//! The database handle: single-writer actor + reader pool.
//!
//! Blueprint ch. 9.2: "The database is a single-writer connection behind a
//! DB actor with watcher channels". Writes are serialized through a
//! `tokio_rusqlite::Connection` (dedicated thread + command channel —
//! the Phase 03 task pins "command channel over tokio-rusqlite"); reads use
//! a small pool of plain connections, which WAL mode lets run concurrently
//! with the writer.
//!
//! Upstream counterpart: Room with `WRITE_AHEAD_LOGGING` and the two fixed
//! 4-thread executors (MusicDatabase.kt:172-177 — query and transaction
//! executors, both `newFixedThreadPool(4)`).
//!
//! The write futures returned here are runtime-agnostic: the actor thread is
//! a plain OS thread and the reply rides a oneshot that needs no tokio
//! context to be polled, so GPUI's executor can await `write()` directly
//! (PORT_SPEC §3.1 rule 4).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use rusqlite::Connection;

use crate::error::{DbError, DbResult};
use crate::schema;
use crate::watch::{InvalidationHandle, TableSet, WatchGuard};

/// Pool of read connections: WAL readers never block the writer
/// (MusicDatabase.kt JournalMode.WRITE_AHEAD_LOGGING).
pub struct ReaderPool {
    idle: Mutex<Vec<Connection>>,
    cond: Condvar,
}

impl ReaderPool {
    fn new(conns: Vec<Connection>) -> Self {
        Self {
            idle: Mutex::new(conns),
            cond: Condvar::new(),
        }
    }

    /// Runs `f` with a pooled read connection, blocking until one is free.
    pub fn with_connection<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        let mut guard = self.idle.lock().expect("reader pool poisoned");
        while guard.is_empty() {
            guard = self.cond.wait(guard).expect("reader pool poisoned");
        }
        let conn = guard.pop().expect("pool had an idle connection");
        drop(guard);
        let result = f(&conn);
        self.idle.lock().expect("reader pool poisoned").push(conn);
        self.cond.notify_one();
        result
    }
}

/// An opened database: writer actor + readers + invalidation bus.
///
/// Clone-free handle: cheap to share via `Arc<Db>`; the repositories hold
/// one `Db` each through `Arc`.
pub struct Db {
    writer: tokio_rusqlite::Connection,
    readers: Arc<ReaderPool>,
    path: PathBuf,
    invalidations: crate::watch::Invalidations,
}

/// How many read connections to open (upstream's fixed pool size).
const READER_POOL_SIZE: usize = 4;

/// Process-global sequence for in-memory database names — see
/// [`Db::open_in_memory`] for why this must not be a clock read.
fn next_memory_db_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Db {
    /// Opens (creating if needed) the database at `path` and ensures the
    /// v45 baseline schema is present.
    ///
    /// Mirrors `InternalDatabase.newInstance` (MusicDatabase.kt:165-192):
    /// WAL journal mode, the onOpen PRAGMAs, and — instead of Room's
    /// migration chain — the desktop baseline DDL at `user_version = 1`.
    pub async fn open(path: impl AsRef<Path>) -> DbResult<Self> {
        let path = path.as_ref().to_path_buf();
        let writer = tokio_rusqlite::Connection::open(&path).await?;

        // Writer setup runs on the actor thread; failures surface as
        // DbError through the closure.
        let setup_path = path.clone();
        writer
            .call(move |conn| {
                schema::apply_pragmas(conn)?;
                schema::apply(conn, &setup_path)?;
                Ok(())
            })
            .await
            .map_err(DbError::from)?;

        let mut readers = Vec::with_capacity(READER_POOL_SIZE);
        for _ in 0..READER_POOL_SIZE {
            let conn = Connection::open(&path)?;
            schema::apply_pragmas(&conn)?;
            readers.push(conn);
        }

        Ok(Self {
            writer,
            readers: Arc::new(ReaderPool::new(readers)),
            path,
            invalidations: crate::watch::Invalidations::new(),
        })
    }

    /// In-memory database for tests: one shared cache so the writer and the
    /// readers see the same data (`file::memory:?cache=shared`).
    pub async fn open_in_memory() -> DbResult<Self> {
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
            | rusqlite::OpenFlags::SQLITE_OPEN_SHARED_CACHE;
        // Unique per-instance URI so parallel tests never share a cache.
        //
        // The name is `pid-counter`, NOT a clock read: macOS `clock_gettime`
        // ticks at microsecond granularity, so two `SystemTime::now()` calls
        // from parallel test threads can return the SAME nanosecond value,
        // which made two tests open the same shared-cache in-memory database
        // (the second `INSERT OR IGNORE` of the same song id then silently
        // returned `Ok(false)` — the macOS CI flake in `tests/watcher.rs`).
        // A process-global atomic counter is unique by construction on every
        // OS; the pid merely keeps the name self-describing.
        let uri = format!(
            "file:mochydb-test-{}-{}?mode=memory&cache=shared",
            std::process::id(),
            next_memory_db_seq(),
        );
        let writer = tokio_rusqlite::Connection::open_with_flags(&uri, flags).await?;
        let setup_uri = uri.clone();
        writer
            .call(move |conn| {
                // WAL is meaningless for :memory:, the rest still applies.
                let _: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
                conn.pragma_update(None, "busy_timeout", 60_000_i64)?;
                conn.pragma_update(None, "cache_size", -16_000_i64)?;
                conn.pragma_update(None, "foreign_keys", "ON")?;
                schema::apply(conn, Path::new(":memory:"))?;
                Ok(())
            })
            .await
            .map_err(DbError::from)?;

        let mut readers = Vec::with_capacity(READER_POOL_SIZE);
        for _ in 0..READER_POOL_SIZE {
            let conn = Connection::open_with_flags(&setup_uri, flags)?;
            conn.pragma_update(None, "busy_timeout", 60_000_i64)?;
            conn.pragma_update(None, "foreign_keys", "ON")?;
            readers.push(conn);
        }

        Ok(Self {
            writer,
            readers: Arc::new(ReaderPool::new(readers)),
            path: PathBuf::from(":memory:"),
            invalidations: crate::watch::Invalidations::new(),
        })
    }

    /// The database file path (`:memory:` for in-memory handles).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs one write transaction on the actor and, after commit, fires the
    /// invalidation for `tables`.
    ///
    /// `f` runs on the writer thread inside `BEGIN … COMMIT`; its error
    /// aborts the transaction. This is the port of Room's
    /// `runInTransaction` + invalidation-tracker semantics
    /// (MusicDatabase.kt transaction helpers + blueprint ch. 7.3).
    pub async fn write<T>(
        &self,
        tables: TableSet,
        f: impl FnOnce(&mut Connection) -> DbResult<T> + Send + 'static,
    ) -> DbResult<T>
    where
        T: Send + 'static,
    {
        let invalidations = self.invalidations.clone();
        // The closure runs on the actor thread; invalidation fires only
        // after COMMIT succeeded (an aborted transaction must not wake
        // queries — Room behaves the same way).
        let result = self
            .writer
            .call(move |conn| {
                // Manual BEGIN/COMMIT (rusqlite's Transaction guards a
                // &mut Connection behind an immutable Deref, while the
                // repository closures take &mut Connection directly).
                conn.execute_batch("BEGIN IMMEDIATE")?;
                match f(conn) {
                    Ok(value) => {
                        conn.execute_batch("COMMIT")?;
                        invalidations.send(tables);
                        Ok(value)
                    }
                    Err(e) => {
                        // Best-effort rollback; the error we keep is the
                        // original one.
                        let _ = conn.execute_batch("ROLLBACK");
                        Err(e)
                    }
                }
            })
            .await
            .map_err(DbError::from)?;
        Ok(result)
    }

    /// Like [`Db::write`], but WITHOUT the wrapping transaction — the
    /// closure manages its own BEGIN/COMMIT (and any pragmas that must sit
    /// outside a transaction, such as `foreign_keys`).
    ///
    /// Reserved for bulk tools (the Android importer); repositories use
    /// [`Db::write`]. Invalidation still fires only after the closure
    /// reports success.
    pub async fn write_raw<T>(
        &self,
        tables: TableSet,
        f: impl FnOnce(&mut Connection) -> DbResult<T> + Send + 'static,
    ) -> DbResult<T>
    where
        T: Send + 'static,
    {
        let invalidations = self.invalidations.clone();
        self.writer
            .call(move |conn| {
                let value = f(conn)?;
                invalidations.send(tables);
                Ok(value)
            })
            .await
            .map_err(DbError::from)
    }

    /// Runs `f` with a pooled read connection (blocking the calling thread).
    ///
    /// SQLite reads over WAL are typically sub-millisecond; async callers
    /// that expect heavier queries wrap this in their executor's blocking
    /// facility. Reads never touch the actor.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        self.readers.with_connection(f)
    }

    /// Subscribes to the raw invalidation broadcast (multi-subscriber,
    /// drop-oldest under lag). See [`InvalidationHandle::recv`].
    pub fn subscribe(&self) -> InvalidationHandle {
        self.invalidations.subscribe()
    }

    /// Subscribes with the mochy-config watcher shape: an mpsc receiver plus
    /// a guard that unsubscribes on drop. Yields the intersecting table
    /// subset of every write touching any watched table.
    pub fn watch(&self, tables: TableSet) -> (std::sync::mpsc::Receiver<TableSet>, WatchGuard) {
        self.invalidations.watch(tables)
    }
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").field("path", &self.path).finish()
    }
}
