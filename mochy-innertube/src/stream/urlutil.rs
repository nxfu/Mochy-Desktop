//! URL helpers for stream URLs — the `android.net.Uri`
//! `encode`/`decode` pair the stream code uses
//! (EjsNTransformSolver.kt:34/53, CipherDeobfuscator.kt:61/83/98,
//! YTPlayerUtils.kt:648).
//!
//! Android's `Uri.encode` percent-encodes everything except the
//! unreserved set `A-Za-z0-9-_.~` and the sub-delims `!'()*`; its
//! `Uri.decode` resolves `%XX` sequences and leaves `+` alone (it is
//! a path/query decoder, not a form decoder).

/// `Uri.encode` — percent-encode for a query component.
pub(crate) fn uri_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'\''
            | b'('
            | b')'
            | b'*' => out.push(*byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// `Uri.decode` — resolve `%XX` sequences; `+` stays `+`; a stray `%`
/// that is not a valid escape is passed through (Android logs and
/// keeps it).
pub(crate) fn uri_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            match bytes
                .get(index + 1..index + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                Some(byte) => {
                    out.push(byte);
                    index += 3;
                }
                None => {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_matches_android_uri() {
        // unreserved and sub-delims pass; everything else escapes
        assert_eq!(uri_encode("abcXYZ09-_.~!'()*"), "abcXYZ09-_.~!'()*");
        assert_eq!(uri_encode("a b"), "a%20b");
        assert_eq!(uri_encode("a+b"), "a%2Bb");
        assert_eq!(uri_encode("t2=w8=f"), "t2%3Dw8%3Df");
    }

    #[test]
    fn decode_resolves_escapes_and_keeps_plus() {
        assert_eq!(uri_decode("abc"), "abc");
        assert_eq!(uri_decode("a%20b"), "a b");
        assert_eq!(uri_decode("a+b"), "a+b", "Uri.decode is not a form decoder");
        assert_eq!(uri_decode("a%2Bb"), "a+b");
        assert_eq!(uri_decode("100%"), "100%", "stray percent passes through");
        assert_eq!(uri_decode("%zz"), "%zz");
    }

    #[test]
    fn round_trips_stream_shaped_values() {
        for value in ["hghUgJWNCI-IBStoNd", "ttPvh7WIptsgSw", "a=b&c"] {
            assert_eq!(uri_decode(&uri_encode(value)), value);
        }
    }
}
