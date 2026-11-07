//! Base64 decoding (RFC 4648's standard alphabet), for `data:` URIs.

/// Each byte's six bits, or `BAD` for a byte outside the alphabet.
const BAD: u8 = 0xff;
const TABLE: [u8; 256] = {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [BAD; 256];
    let mut i = 0;
    while i < 64 {
        table[alphabet[i] as usize] = i as u8;
        i += 1;
    }
    table
};

/// The bytes `text` encodes; none when it holds a byte outside the alphabet
/// or has a length no encoding has. Padding (`=`) is optional.
pub(crate) fn decode(text: &[u8]) -> Option<Vec<u8>> {
    let text = text
        .strip_suffix(b"==")
        .or(text.strip_suffix(b"="))
        .unwrap_or(text);
    let tail = text.len() % 4;
    if tail == 1 {
        return None;
    }
    let whole = text.len() - tail;
    let mut out = vec![0; whole / 4 * 3 + tail.saturating_sub(1)];
    let (body, rest) = out.split_at_mut(whole / 4 * 3);
    let mut bad = 0;
    for (quad, triple) in text[..whole]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(body.as_chunks_mut::<3>().0)
    {
        let [a, b, c, d] = quad.map(|byte| TABLE[byte as usize]);
        bad |= a | b | c | d;
        let bits = u32::from(a) << 18 | u32::from(b) << 12 | u32::from(c) << 6 | u32::from(d);
        triple.copy_from_slice(&bits.to_be_bytes()[1..]);
    }
    if tail > 0 {
        let mut bits = 0u32;
        for (i, &byte) in text[whole..].iter().enumerate() {
            let six = TABLE[byte as usize];
            bad |= six;
            bits |= u32::from(six) << (18 - 6 * i);
        }
        rest.copy_from_slice(&bits.to_be_bytes()[1..tail]);
    }
    (bad & 0x80 == 0).then_some(out)
}

#[cfg(test)]
pub(crate) fn encode(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let bits = chunk
            .iter()
            .enumerate()
            .fold(0u32, |bits, (i, &b)| bits | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(match i <= chunk.len() {
                true => alphabet[(bits >> (18 - 6 * i) & 63) as usize] as char,
                false => '=',
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_length_reads_back_with_or_without_padding() {
        let bytes: Vec<u8> = (0..=255).rev().collect();
        for length in 0..40 {
            let text = encode(&bytes[..length]);
            assert_eq!(decode(text.as_bytes()).as_deref(), Some(&bytes[..length]));
            let bare = text.trim_end_matches('=');
            assert_eq!(decode(bare.as_bytes()).as_deref(), Some(&bytes[..length]));
        }
        assert_eq!(decode(b"TWFu").as_deref(), Some(&b"Man"[..]));
    }

    #[test]
    fn bytes_outside_the_alphabet_and_impossible_lengths_are_refused() {
        assert_eq!(decode(b"TWF"), Some(b"Ma".to_vec()));
        assert_eq!(decode(b"T"), None);
        assert_eq!(decode(b"TW-u"), None);
        assert_eq!(decode(b"TWFu TWFu"), None);
        assert_eq!(decode(b"TWF\0"), None);
    }
}
