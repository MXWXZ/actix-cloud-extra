use base64::{Engine as _, prelude::BASE64_STANDARD};
use infer::Type;

pub trait StringUtil {
    /// Check whether the string is a non-empty ASCII hex string.
    fn is_hex(&self) -> bool;

    /// Escape `like` SQL in `s`.
    /// Replace `\` to `\\`, `%` to `\%` and `_` to `\_`.
    fn like_escape(&self) -> String;
}

impl<S> StringUtil for S
where
    S: AsRef<str>,
{
    /// Check whether the string is a non-empty ASCII hex string.
    fn is_hex(&self) -> bool {
        let s = self.as_ref();
        !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
    }

    /// Escape `like` SQL in string.
    fn like_escape(&self) -> String {
        let mut s = self.as_ref().replace('\\', "\\\\"); // first replace \
        s = s.replace('%', "\\%");
        s.replace('_', "\\_")
    }
}

/// A decoded data URL: raw bytes plus the MIME type inferred from them.
///
/// No size limiting or MIME validation is performed. Callers handling
/// untrusted input must enforce a maximum encoded/decoded length and apply a
/// MIME allowlist before serving the content; the inferred [`Type`] is only a
/// hint.
pub struct DataUrl {
    pub data: Vec<u8>,
    pub mime: Type,
}

impl DataUrl {
    /// Encode as `data:{mime};base64,{payload}`.
    pub fn encode(&self) -> String {
        let data = BASE64_STANDARD.encode(&self.data);
        format!("data:{};base64,{data}", self.mime)
    }

    /// Decode a data URL.
    ///
    /// The prefix before the comma is ignored; in particular `data:` and
    /// `;base64` are not validated, so a successful decode does not prove the
    /// input was a data URL. The returned MIME type is inferred from the
    /// decoded bytes and should be treated as an untrusted hint, not as the
    /// declared type.
    ///
    /// The input is not size-limited. When handling untrusted data, enforce a
    /// maximum length before calling this method: decoding allocates memory
    /// proportional to the payload size.
    pub fn decode<S: AsRef<str>>(data: S) -> Option<Self> {
        let data: Vec<_> = data.as_ref().split(',').collect();
        if data.len() == 2 {
            BASE64_STANDARD.decode(data[1]).ok().and_then(Self::new)
        } else {
            None
        }
    }

    /// Infer the MIME type from `data`, returning `None` when it is not
    /// recognized.
    ///
    /// No size limit is applied; the caller owns any input-size policy.
    pub fn new(data: Vec<u8>) -> Option<Self> {
        let mime = infer::get(&data);
        mime.map(|mime| Self { data, mime })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    }

    // ---- normal path ----

    #[test]
    fn normal_is_hex_mixed_case() {
        assert!("0123456789abcdefABCDEF".is_hex());
        assert!("abcdef".is_hex());
        assert!("ABCDEF".is_hex());
    }

    #[test]
    fn boundary_is_hex_rejects_empty() {
        assert!(!"".is_hex());
    }

    #[test]
    fn normal_like_escape_plain() {
        assert_eq!("plain".like_escape(), "plain");
    }

    #[test]
    fn normal_like_escape_specials() {
        assert_eq!("a%b_c\\d".like_escape(), "a\\%b\\_c\\\\d");
    }

    #[test]
    fn normal_dataurl_png_roundtrip() {
        let url = DataUrl::new(png()).unwrap();
        let encoded = url.encode();
        let decoded = DataUrl::decode(&encoded).unwrap();
        assert_eq!(decoded.data, png());
        assert_eq!(decoded.mime.mime_type(), "image/png");
    }

    #[test]
    fn normal_dataurl_encode_exact() {
        let url = DataUrl::new(png()).unwrap();
        assert_eq!(url.encode(), "data:image/png;base64,iVBORw0KGgo=");
    }

    #[test]
    fn normal_dataurl_gif_detection() {
        let url = DataUrl::new(b"GIF89a".to_vec()).unwrap();
        assert_eq!(url.mime.mime_type(), "image/gif");
    }

    #[test]
    fn normal_dataurl_decode_ignores_declared_mime() {
        // The prefix before the comma is never validated; the MIME type comes
        // from the decoded bytes.
        let declared = DataUrl::decode("data:text/plain;base64,iVBORw0KGgo=").unwrap();
        assert_eq!(declared.mime.mime_type(), "image/png");
        let garbage = DataUrl::decode("whatever,iVBORw0KGgo=").unwrap();
        assert_eq!(garbage.mime.mime_type(), "image/png");
    }

    // ---- boundary ----

    #[test]
    fn boundary_is_hex_rejects_prefix_whitespace_unicode() {
        assert!(!"0x123".is_hex());
        assert!(!" 12".is_hex());
        assert!(!"12\n".is_hex());
        assert!(!"\u{FF11}\u{FF12}".is_hex()); // fullwidth digits
        assert!(!"é".is_hex());
        assert!(!"+1".is_hex());
    }

    #[test]
    fn boundary_like_escape_empty_and_specials_only() {
        assert_eq!("".like_escape(), "");
        assert_eq!("%_\\".like_escape(), "\\%\\_\\\\");
    }

    #[test]
    fn boundary_like_escape_consecutive_and_already_escaped() {
        assert_eq!("%%__".like_escape(), "\\%\\%\\_\\_");
        // Escaping is not idempotent: input backslashes are escaped too.
        assert_eq!("\\%".like_escape(), "\\\\\\%");
    }

    #[test]
    fn boundary_dataurl_decode_malformed() {
        assert!(DataUrl::decode("no-comma-here").is_none());
        assert!(DataUrl::decode("a,b,c").is_none());
        assert!(DataUrl::decode("data:image/png;base64,!!!!").is_none());
        assert!(DataUrl::decode("data:image/png;base64,--__").is_none());
        assert!(DataUrl::decode("data:image/png;base64,iVBOR w0KGgo=").is_none());
        assert!(DataUrl::decode(",").is_none());
    }

    #[test]
    fn boundary_dataurl_decode_unknown_or_empty_payload() {
        // Valid base64, but the bytes have no detectable MIME.
        assert!(DataUrl::decode("x,AQID").is_none());
        assert!(DataUrl::decode("data:image/png;base64,").is_none());
    }

    // ---- extreme ----

    #[test]
    fn extreme_is_hex_long_input() {
        assert!("a".repeat(10_000).is_hex());
        let bad = format!("{}g", "a".repeat(9_999));
        assert!(!bad.is_hex());
    }

    #[test]
    fn extreme_like_escape_backslash_first_and_unicode() {
        // `\` must be escaped first so it does not double-escape later inserts.
        assert_eq!("%\\".like_escape(), "\\%\\\\");
        assert_eq!("中文%\\_".like_escape(), "中文\\%\\\\\\_");
        assert_eq!("\n\t".like_escape(), "\n\t");
    }

    #[test]
    fn extreme_dataurl_large_roundtrip() {
        let mut data = png();
        data.resize(64 * 1024, 0xAB);
        let url = DataUrl::new(data.clone()).unwrap();
        let decoded = DataUrl::decode(url.encode()).unwrap();
        assert_eq!(decoded.data, data);
        assert_eq!(decoded.mime.mime_type(), "image/png");
    }

    #[test]
    fn dataurl_new_unknown() {
        // Empty / random bytes have no detectable mime.
        assert!(DataUrl::new(Vec::new()).is_none());
        assert!(DataUrl::new(vec![0x01, 0x02, 0x03]).is_none());
    }
}
