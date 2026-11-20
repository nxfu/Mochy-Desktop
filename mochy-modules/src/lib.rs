//! Module trait + registry, JSON web/local modules, skin DSL parser, route takeover model
//!
//! Upstream absorption: modules/api + modules/bridge (MODULE_API_VERSION 3) + app modules/ (29 files: ModuleRegistry, SourceModules routing, skin DSL 14 files, takeovers).
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-modules");
    }
}
