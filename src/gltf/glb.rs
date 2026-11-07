//! GLB, glTF's binary container: a 12-byte header, a JSON chunk and an
//! optional binary chunk, each padded to four bytes.
use super::GltfError;

const MAGIC: &[u8; 4] = b"glTF";
const JSON: u32 = 0x4e4f_534a;
const BIN: u32 = 0x004e_4942;

/// Whether `bytes` start as a GLB.
pub(crate) fn is_glb(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// A GLB's JSON text and binary chunk (unknown chunks skipped).
pub(crate) fn split(bytes: &[u8]) -> Result<(&[u8], Option<&[u8]>), GltfError> {
    let malformed = |message: &str| GltfError::malformed("GLB", message);
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|w| u32::from_le_bytes(w.try_into().expect("four bytes")))
    };
    let (Some(version), Some(length)) = (word(4), word(8)) else {
        return Err(malformed("the header is cut short"));
    };
    if version != 2 {
        return Err(malformed(&format!("version {version}, where 2 is read")));
    }
    let length = length as usize;
    if length > bytes.len() {
        return Err(malformed(&format!(
            "{length} bytes declared, {} present",
            bytes.len()
        )));
    }
    let (mut json, mut bin) = (None, None);
    let mut at = 12;
    while at < length {
        let (Some(size), Some(kind)) = (word(at), word(at + 4)) else {
            return Err(malformed("a chunk header is cut short"));
        };
        let start = at + 8;
        let end = start
            .checked_add(size as usize)
            .filter(|&end| end <= length);
        let chunk = &bytes[start..end.ok_or_else(|| malformed("a chunk runs past the end"))?];
        match kind {
            JSON if at == 12 => json = Some(chunk),
            BIN if at > 12 && bin.is_none() => bin = Some(chunk),
            JSON | BIN => return Err(malformed("chunks out of order")),
            _ => {}
        }
        at = start + chunk.len().next_multiple_of(4);
    }
    let json = json.ok_or_else(|| malformed("no JSON chunk"))?;
    Ok((json, bin))
}

/// A GLB of `json` (padded with spaces) and `bin` (padded with zeros; no
/// chunk when empty).
pub(crate) fn join(json: &str, bin: &[u8]) -> Vec<u8> {
    let json_size = json.len().next_multiple_of(4);
    let bin_size = bin.len().next_multiple_of(4);
    let total = 12 + 8 + json_size + if bin.is_empty() { 0 } else { 8 + bin_size };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(MAGIC);
    for word in [2, total as u32, json_size as u32, JSON] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(json.as_bytes());
    out.resize(20 + json_size, b' ');
    if !bin.is_empty() {
        for word in [bin_size as u32, BIN] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(bin);
        out.resize(total, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_container_splits_into_what_was_joined() {
        let glb = join("{\"a\":1}", &[1, 2, 3, 4, 5]);
        assert_eq!(glb.len(), 12 + 8 + 8 + 8 + 8);
        let (json, bin) = split(&glb).unwrap();
        assert_eq!(json, b"{\"a\":1} ");
        assert_eq!(bin, Some(&[1, 2, 3, 4, 5, 0, 0, 0][..]));
        assert_eq!(split(&join("{}", &[])).unwrap().1, None);
    }

    #[test]
    fn cut_or_damaged_containers_are_errors() {
        let glb = join("{}", &[7; 9]);
        for length in 0..glb.len() {
            let mut cut = glb[..length].to_vec();
            if length >= 12 {
                cut[8..12].copy_from_slice(&(length as u32).to_le_bytes());
            }
            // At 24 bytes the JSON chunk is whole: a GLB without a binary chunk.
            assert_eq!(split(&cut).is_ok(), length == 24, "{length}");
        }
        let mut version = glb.clone();
        version[4] = 1;
        assert!(split(&version).is_err());
    }
}
