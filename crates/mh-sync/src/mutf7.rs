//! IMAP kutu adları için değiştirilmiş UTF-7 (RFC 3501 §5.1.3).

use base64::Engine as _;

/// "G&APY-nderilmi&AV8-" → "Gönderilmiş". Bozuk dizi olduğu gibi kalır.
pub fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let chunk = &tail[..end];
        if chunk.is_empty() {
            out.push('&');
        } else {
            match decode_chunk(chunk) {
                Some(d) => out.push_str(&d),
                None => out.push_str(&rest[start..start + 1 + end + 1]),
            }
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

fn decode_chunk(chunk: &str) -> Option<String> {
    let mut b64 = chunk.replace(',', "/");
    while !b64.len().is_multiple_of(4) {
        b64.push('=');
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// Ters dönüşüm (yeni kutu oluştururken).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending: Vec<u16> = Vec::new();
    let flush = |pending: &mut Vec<u16>, out: &mut String| {
        if pending.is_empty() {
            return;
        }
        let bytes: Vec<u8> = pending.iter().flat_map(|u| u.to_be_bytes()).collect();
        let enc = base64::engine::general_purpose::STANDARD_NO_PAD
            .encode(bytes)
            .replace('/', ",");
        out.push('&');
        out.push_str(&enc);
        out.push('-');
        pending.clear();
    };
    for c in s.chars() {
        if (' '..='~').contains(&c) {
            flush(&mut pending, &mut out);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut buf = [0u16; 2];
            pending.extend_from_slice(c.encode_utf16(&mut buf));
        }
    }
    flush(&mut pending, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkce_kutu_adlari() {
        for name in [
            "Gönderilmiş Öğeler",
            "Çöp Kutusu",
            "İstenmeyen",
            "Taslaklar",
            "A&B",
            "日本語",
        ] {
            let enc = encode(name);
            assert!(enc.is_ascii(), "{enc}");
            assert_eq!(decode(&enc), name);
        }
        assert_eq!(decode("INBOX"), "INBOX");
        assert_eq!(decode("&-"), "&");
        assert_eq!(decode("bozuk&@@@-son"), "bozuk&@@@-son");
        assert_eq!(decode("eksik&AV8"), "eksik&AV8");
    }
}
