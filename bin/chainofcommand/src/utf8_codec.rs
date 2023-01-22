use {bitmatch::bitmatch, bytes::BytesMut, tokio_util::codec::Decoder};

/// Decode byte-stream into UTF-8 chars.
pub struct Utf8Codec;

impl Utf8Codec {
    pub fn new() -> Self {
        Self
    }
}

// impl Encoder for Utf8Codec {}

enum DecodeByte {
    Single(char),
    Start(usize, u32),
    Continue(u32),
    Invalid(u8),
}

#[bitmatch]
fn unpack_decode_byte(decode_byte: u8) -> DecodeByte {
    #[bitmatch]
    match decode_byte {
        "1111_0xxx" => DecodeByte::Start(3, x.into()), // 4 bytes
        "1110_xxxx" => DecodeByte::Start(2, x.into()), // 3 bytes
        "110x_xxxx" => DecodeByte::Start(1, x.into()), // 2 bytes
        "10xx_xxxx" => DecodeByte::Continue(x.into()), // follow up byte
        "0xxx_xxxx" => DecodeByte::Single(x.into()),   // 1 byte
        _ => DecodeByte::Invalid(decode_byte),
    }
}

// TODO: try Utf8Chunks https://doc.rust-lang.org/src/core/str/lossy.rs.html#145-147
// TODO: try enforcing code point length bytes available in the buffer prior to attempting decode?

impl Decoder for Utf8Codec {
    type Item = char;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut chunks = std::str::Utf8Chunks::new(src);
        if let Some(chunk) = chunks.next() {
            if chunk.valid().len() > 0 {
                let first = chunk.valid().chars().nth(0).unwrap();
                let _ = src.split_to(first.len_utf8());
                return Ok(Some(first));
            }
            // additional check: if sequence start is for longer than the remaining buffer,
            // wait for more.

            let _ = src.split_to(chunk.invalid().len());
            return Ok(Some(char::REPLACEMENT_CHARACTER)); // todo
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use {crate::utf8_codec::Utf8Codec, bytes::BytesMut, tokio_util::codec::Decoder};

    #[test]
    fn fully_decode_correct_utf8() {
        let correct = vec![0x7a, 0xc2, 0xa7, 0xf0, 0x9f, 0xa4, 0xa3];
        // Shall yield 3 valid points from the stream
        let mut decoder = Utf8Codec::default();
        let mut buf = BytesMut::from(&correct[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('z'));
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('§'));
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('🤣'));
    }
    #[test]
    fn decode_incorrect_utf8() {
        let incorrect = vec![0x7a, 0xc2, 0xa7, 0xf0, 0xf0, 0xf0, 0xf0];
        // Shall yield 2 valid points and then 4 replacement chars
        let mut decoder = Utf8Codec::default();
        let mut buf = BytesMut::from(&incorrect[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('z'));
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('§'));
        assert_eq!(
            decoder.decode(&mut buf).unwrap(),
            Some(char::REPLACEMENT_CHARACTER)
        );
        assert_eq!(
            decoder.decode(&mut buf).unwrap(),
            Some(char::REPLACEMENT_CHARACTER)
        );
        assert_eq!(
            decoder.decode(&mut buf).unwrap(),
            Some(char::REPLACEMENT_CHARACTER)
        );
        assert_eq!(
            decoder.decode(&mut buf).unwrap(),
            Some(char::REPLACEMENT_CHARACTER)
        );
        assert_eq!(decoder.decode(&mut buf).unwrap(), None);
    }
    #[test]
    fn recover_after_incorrect_utf8() {
        let incorrect = vec![0x7a, 0xc2, 0xa7, 0xf0, 0xf0, 0x7a];
        // Shall yield 2 valid points, then two replacement chars then another valid point
        let mut decoder = Utf8Codec::default();
        let mut buf = BytesMut::from(&incorrect[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('z'));
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('§'));
        assert_eq!(
            decoder.decode(&mut buf).unwrap(),
            Some(char::REPLACEMENT_CHARACTER)
        );
        assert_eq!(
            decoder.decode(&mut buf).unwrap(),
            Some(char::REPLACEMENT_CHARACTER)
        );
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('z'));
    }
    #[test]
    fn partially_decode_correct_utf8() {
        let incomplete = vec![0x7a, 0xc2];
        let mut decoder = Utf8Codec::default();
        let mut buf = BytesMut::from(&incomplete[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('z'));
        assert_eq!(decoder.decode(&mut buf).unwrap(), None); // incomplete
    }
    #[test]
    fn partially_decode_incorrect_utf8() {
        let incomplete_invalid = vec![0x7a, 0xf0, 0xf0];
        let mut decoder = Utf8Codec::default();
        let mut buf = BytesMut::from(&incomplete_invalid[..]);
        assert_eq!(decoder.decode(&mut buf).unwrap(), Some('z'));
        assert_eq!(decoder.decode(&mut buf).unwrap(), None); // incomplete
    }
}
