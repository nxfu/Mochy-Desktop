//! Pairing, LAN server/client, remote control, TV profiles - byte-compatible protocol
//!
//! Upstream absorption: link/ (26 files: mDNS advertising, ECDH secp256r1 pairing + 6-digit code, HMAC HTTP/1.1 with nonces, gzipped SQLite snapshots, ranged file streaming).
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-link");
    }
}
