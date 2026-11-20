//! Account/link/CSV/local importers, TrackMatcher, TransferArchive, YT sync engine
//!
//! Upstream absorption: importer/ (42 files: Spotify/Apple/Tidal accounts, LinkImporter, CsvImport, LocalImportService, transfer/) + utils/SyncUtils (YT two-way sync).
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-importer");
    }
}
