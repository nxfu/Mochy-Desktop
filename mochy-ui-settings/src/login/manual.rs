//! The manual cookie-paste dialog — tier 3, the R3 last resort.
//!
//! Not in the pinned upstream (Android's webview always carries the
//! UA-stripped login; the desktop R3 mitigation adds this for the
//! machines where even the loopback tier cannot finish): the user
//! signs into music.youtube.com in their own browser, copies the
//! `Cookie:` request header (any devtools cookie exporter shape), and
//! pastes it. Validation is exactly `finishLogin`'s guard
//! (YouTubeLoginScreen.kt:165-166): non-empty, parses, carries
//! SAPISID. The visitor/data-sync ids cannot come from a paste — the
//! session mirror's guest scrape fills `visitorData` and
//! `DATASYNC_ID` stays absent until a real sign-in or a ytcfg-bearing
//! page provides one (the innertube clients work without it; upstream
//! treats both as optional everywhere it reads them).

use super::account::{LoginCredentials, LoginError};

/// Validate a pasted cookie header — the tier-3 counterpart of the
/// harvest. Whitespace-tolerant: pastes arrive wrapped in newlines.
pub fn validate_paste(paste: &str) -> Result<LoginCredentials, LoginError> {
    LoginCredentials::validate(paste)
}

/// Normalize a paste for the dialog's preview: one `name=value` pair
/// per comma/space-separated group collapsed to the canonical
/// `a=1; b=2` shape. Accepts both `;` and newline separators (the
/// devtools "copy all cookies" shapes vary).
pub fn normalize_paste(paste: &str) -> String {
    paste
        .split([';', '\n'])
        .map(str::trim)
        .filter(|part| !part.is_empty() && part.contains('='))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same guard as the harvest: SAPISID decides.
    #[test]
    fn paste_requires_sapisid() {
        assert!(validate_paste("SID=x; SAPISID=y").is_ok());
        assert!(matches!(
            validate_paste("SID=x; HSID=y"),
            Err(LoginError::MissingSapisid)
        ));
        assert!(matches!(
            validate_paste("\n\n   \n"),
            Err(LoginError::EmptyCookie)
        ));
    }

    /// The devtools paste shapes normalize to the stored shape.
    #[test]
    fn normalize_handles_both_paste_shapes() {
        assert_eq!(
            normalize_paste("SAPISID=abc; SID=xyz;"),
            "SAPISID=abc; SID=xyz"
        );
        assert_eq!(
            normalize_paste("SAPISID=abc\nSID=xyz\n__Secure-3PAPISID=q"),
            "SAPISID=abc; SID=xyz; __Secure-3PAPISID=q"
        );
        assert_eq!(normalize_paste("  "), "");

        // a normalized paste must validate
        let normalized = normalize_paste("SAPISID=abc\nSID=xyz");
        assert!(validate_paste(&normalized).is_ok());
    }
}
