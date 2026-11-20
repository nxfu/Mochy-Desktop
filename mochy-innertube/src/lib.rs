//! 18-client registry, endpoints, pagination, auth headers, PO tokens, EJS n-solver, stream-format ranking
//!
//! Upstream absorption: providers/innertube (106 files: InnerTube.kt transport, YouTube.kt 1,814-line facade, models/, pages/ 21 parsers + 2 glue) + app utils/YTPlayerUtils.kt (1,241 lines) + potoken/ + cipher/ + sabr/.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-innertube");
    }
}
