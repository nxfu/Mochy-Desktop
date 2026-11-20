//! Discord gateway, scrobbler, Chromecast CASTv2, DLNA, canvas providers, Shazam, Wikipedia
//!
//! Upstream absorption: discord/ (17 files) + scrobble/ (5) + recognition/ (4) + foss cast/ (CASTv2 TLS + protobuf frames, DLNA SOAP + SSDP) + providers/{lastfm,canvas,applecanvas,artistvideo,shazamkit,wikipedia,genius}.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-integrations");
    }
}
