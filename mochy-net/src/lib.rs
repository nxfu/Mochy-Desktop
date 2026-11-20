//! reqwest factory with proxy/DNS/IP-version policy, response cache, offline flag
//!
//! Upstream absorption: network utils: AppProxy (SOCKS/HTTP + auth), IpVersion pinning, okhttp-dnsoverhttps (DoH), OfflineMode/DataSaver gates, 50 MB disk cache.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-net");
    }
}
