//! GPUI entry, window set (main, TV mode, floating lyrics, webview children), services builder, action/keymap registry, deep links, launch gate, single-instance
//!
//! Upstream absorption: App.kt staged init, MainActivity.kt (NavHost + bottom nav + player sheet + deep-link handling), NavigationBuilder.kt, ui/launch/ (LaunchIntro), the CompositionLocal fabric (replaced by a typed AppContext).
//!
//! Phase 01 bootstrap stub: the GPUI window set, services builder and
//! action/keymap registry land in Phase 04 (M0 checkpoint) — see
//! docs/PORT_SPEC.md "Roadmap" and docs/PROGRESS.md.
//!
//! Phase 07 adds `links` — the pure YouTube link/share parser (the
//! MainActivity.kt 1597-1684 semantics); the window/router wiring that
//! consumes it arrives with the deep-link phase.

mod links;

fn main() {
    // Intentionally tiny: Phase 01 only proves the workspace seam builds.
    // Phase 04 replaces this with the GPUI Application::new() shell.
    println!(
        "mochy (desktop) v{} — bootstrap skeleton; UI lands in Phase 04",
        env!("CARGO_PKG_VERSION")
    );

    // Phase 07: the deep-link entry seam. On Android an intent (or a
    // shared-text EXTRA_TEXT) reaches MainActivity.handleDeepLinkIntent
    // (MainActivity.kt:1597-1684); on desktop the OS scheme registration
    // and single-instance hand-off (Phase 14) deliver the same strings as
    // command-line arguments. Files opened through the file manager
    // ("open with Mochy") are the ACTION_SEND/EXTRA_STREAM analog
    // (importSharedAudio, MainActivity.kt:1565-1595) and arrive as
    // existing paths. Until the router exists, every argument is parsed
    // and printed — proving the seam end to end (and giving the parity
    // harness a probe: `mochy 'https://youtu.be/<id>'`).
    let (files, links): (Vec<String>, Vec<String>) = std::env::args()
        .skip(1)
        .partition(|arg| std::path::Path::new(arg).exists());
    if let Some(outcome) = links::parse_shared_files(&files) {
        println!("share: {outcome:?}");
    }
    for arg in links {
        match links::parse(&arg) {
            Some(outcome) => println!("link: {arg} -> {outcome:?}"),
            None => println!("link: {arg} -> (nothing upstream would do)"),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy");
    }
}
