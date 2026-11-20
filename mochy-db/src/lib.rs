//! rusqlite schema v45 mirror, repositories, invalidation events, Android-DB importer
//!
//! Upstream absorption: db/ (39 files: 24 entities, 3 views, mega-DAO 2,131 lines, 14 hand-written migrations) + di/AppModule DB + app/schemas/ lineage.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-db");
    }
}
