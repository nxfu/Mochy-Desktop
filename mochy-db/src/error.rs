//! Error type for mochy-db.

use std::path::PathBuf;

/// Every failure mochy-db can surface.
///
/// Upstream counterpart: Room wraps SQLite errors in
/// `androidx.sqlite`/`android.database.SQLException` and the app logs them via
/// Timber; the port keeps them typed instead (blueprint ch. 9.1: domain crates
/// expose typed errors, not string logs).
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// A SQLite call failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// The single-writer actor connection was closed.
    #[error("database writer connection closed")]
    WriterClosed,

    /// The schema in the file is not one this crate understands.
    #[error("unsupported schema state at {path:?}: {reason}")]
    BadSchema {
        /// Database file whose `user_version`/tables were unexpected.
        path: PathBuf,
        /// What exactly was wrong.
        reason: String,
    },

    /// An Android source database cannot be imported.
    #[error("cannot import android database at {path:?}: {reason}")]
    Import {
        /// The Android database file.
        path: PathBuf,
        /// Why it was rejected.
        reason: String,
    },

    /// A value could not be converted to/from a row.
    #[error("row conversion failed: {0}")]
    Conversion(String),
}

impl From<tokio_rusqlite::Error<DbError>> for DbError {
    fn from(value: tokio_rusqlite::Error<DbError>) -> Self {
        match value {
            // The repository closure returned a DbError — pass it through.
            tokio_rusqlite::Error::Error(e) => e,
            // The writer thread is gone (Db dropped or close() raced a write).
            tokio_rusqlite::Error::ConnectionClosed => DbError::WriterClosed,
            // Only produced by close(); a failed close still closed the actor.
            tokio_rusqlite::Error::Close(_) => DbError::WriterClosed,
            // The enum is #[non_exhaustive]; unknown future variants can
            // only mean the actor is unusable.
            _ => DbError::WriterClosed,
        }
    }
}

/// Result alias used throughout the crate.
pub type DbResult<T> = Result<T, DbError>;
