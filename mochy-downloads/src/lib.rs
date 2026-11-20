//! Job queue, itag-stability resume, auto-download rules, export remux/tagging
//!
//! Upstream absorption: DownloadUtil + ExoDownloadService + AutoDownloads + DownloadExporter + remux (WebM->Ogg-Opus, MP4->FLAC) + StoragePlaces/StorageMover.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-downloads");
    }
}
