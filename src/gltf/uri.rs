//! What a glTF `uri` names: bytes in a `data:` URI, or a file by a path
//! relative to the glTF file; and what an image's bytes are.
use super::base64;

/// A `uri` member, sorted.
#[derive(Debug, PartialEq)]
pub(crate) enum Uri<'a> {
    /// A base64 `data:` URI: its media type (empty when it gives none) and
    /// its base64 text.
    Data { mime: &'a str, base64: &'a str },
    /// A path, percent-decoded: relative to the file, or absolute. Where it
    /// may lead is the reader's to decide, on the decoded text.
    Path(String),
    /// A URI of another scheme (`http:`, `file:`, a drive's `C:`).
    Other,
}

/// Sorts `uri`; none for a `data:` URI that isn't base64, or bad
/// percent-escapes.
pub(crate) fn sort(uri: &str) -> Option<Uri<'_>> {
    if let Some(data) = uri.strip_prefix("data:") {
        let (header, base64) = data.split_once(',')?;
        let mime = header.strip_suffix(";base64")?;
        return Some(Uri::Data { mime, base64 });
    }
    let scheme = uri
        .find(':')
        .is_some_and(|colon| !uri[..colon].contains('/'));
    match scheme {
        true => Some(Uri::Other),
        false => percent_decoded(uri).map(Uri::Path),
    }
}

/// The bytes of a base64 `data:` URI's text.
pub(crate) fn data(base64: &str) -> Option<Vec<u8>> {
    base64::decode(base64.as_bytes())
}

fn percent_decoded(uri: &str) -> Option<String> {
    if !uri.contains('%') {
        return Some(uri.to_owned());
    }
    let mut bytes = Vec::with_capacity(uri.len());
    let mut rest = uri.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(after.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &after[2..];
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    String::from_utf8(bytes).ok()
}

/// A relative path as a `uri`: characters a URI can't hold percent-escaped,
/// `/` kept.
pub(crate) fn percent_encoded(path: &str) -> String {
    let mut uri = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// The media type an image's first bytes say: PNG, JPEG, KTX2, WebP, AVIF.
pub(crate) fn sniff(bytes: &[u8]) -> Option<&'static str> {
    Some(match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xff, 0xd8, 0xff, ..] => "image/jpeg",
        [0xab, b'K', b'T', b'X', b' ', b'2', b'0', 0xbb, ..] => "image/ktx2",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => "image/webp",
        [
            _,
            _,
            _,
            _,
            b'f',
            b't',
            b'y',
            b'p',
            b'a',
            b'v',
            b'i',
            b'f' | b's',
            ..,
        ] => "image/avif",
        _ => return None,
    })
}

/// The media type a file name's extension says.
pub(crate) fn by_extension(path: &str) -> Option<&'static str> {
    let (_, extension) = path.rsplit_once('.')?;
    Some(match extension.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ktx2" => "image/ktx2",
        "webp" => "image/webp",
        "avif" => "image/avif",
        _ => return None,
    })
}

/// The file extension for a media type.
pub(crate) fn extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/ktx2" => "ktx2",
        "image/webp" => "webp",
        "image/avif" => "avif",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_sort_into_data_and_relative_paths() {
        assert_eq!(
            sort("data:application/octet-stream;base64,AAAA"),
            Some(Uri::Data {
                mime: "application/octet-stream",
                base64: "AAAA"
            })
        );
        assert_eq!(
            sort("data:;base64,"),
            Some(Uri::Data {
                mime: "",
                base64: ""
            })
        );
        assert_eq!(sort("data:text/plain,hello"), None);
        assert_eq!(
            sort("textures/a%20b.png"),
            Some(Uri::Path("textures/a b.png".into()))
        );
        assert_eq!(
            sort("../shared/c.bin"),
            Some(Uri::Path("../shared/c.bin".into()))
        );
        assert_eq!(
            sort("dir/odd:name.png"),
            Some(Uri::Path("dir/odd:name.png".into()))
        );
        for other in ["http://example.com/a.bin", "file:///a.bin", "C:\\a.bin"] {
            assert_eq!(sort(other), Some(Uri::Other), "{other}");
        }
        assert_eq!(sort("a%2"), None);
        // Decoded before anything reads where it leads.
        assert_eq!(sort("%2Fetc%2Fhosts"), Some(Uri::Path("/etc/hosts".into())));
        assert_eq!(sort("/a.bin"), Some(Uri::Path("/a.bin".into())));
    }

    #[test]
    fn paths_escape_and_read_back() {
        let path = "textures/wood albedo#1é.png";
        let uri = percent_encoded(path);
        assert_eq!(uri, "textures/wood%20albedo%231%C3%A9.png");
        assert_eq!(sort(&uri), Some(Uri::Path(path.into())));
    }
}
