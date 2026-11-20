//! GPUI entry, window set (main, TV mode, floating lyrics, webview children), services builder, action/keymap registry, deep links, launch gate, single-instance
//!
//! Upstream absorption: App.kt staged init, MainActivity.kt (NavHost + bottom nav + player sheet + deep-link handling), NavigationBuilder.kt, ui/launch/ (LaunchIntro), the CompositionLocal fabric (replaced by a typed AppContext).
//!
//! Phase 01 bootstrap stub: the GPUI window set, services builder and
//! action/keymap registry land in Phase 04 (M0 checkpoint) — see
//! docs/PORT_SPEC.md "Roadmap" and docs/PROGRESS.md.

fn main() {
    // Intentionally tiny: Phase 01 only proves the workspace seam builds.
    // Phase 04 replaces this with the GPUI Application::new() shell.
    println!(
        "mochy (desktop) v{} — bootstrap skeleton; UI lands in Phase 04",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy");
    }
}
