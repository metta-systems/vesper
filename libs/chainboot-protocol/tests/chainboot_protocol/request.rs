use vesper_chainboot_protocol::*;

#[test]
fn a_request_for_two_megabaud_is_these_bytes() {
    // 2_000_000 = 0x001e_8480; 0x80 + 0x84 + 0x1e + 0x00 = 0x122, so the check is 0xde.
    assert_eq!(
        encode_rate_request(2_000_000),
        [
            b'B', b'A', b'U', b'D', b'R', b'A', b'T', b'E', 0x80, 0x84, 0x1e, 0x00, 0xde
        ]
    );
}

#[test]
fn the_rate_bytes_and_the_check_add_up_to_zero() {
    for rate in [0, 1, 115_200, 921_600, 1_500_000, 2_000_000, u32::MAX] {
        let request = encode_rate_request(rate);
        let sum = request[8..]
            .iter()
            .fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
        assert_eq!(sum, 0, "rate {rate}");
    }
}

#[test]
fn a_request_decodes_to_its_rate() {
    for rate in [115_200, 921_600, 1_500_000, 2_000_000, u32::MAX] {
        let request = encode_rate_request(rate);
        assert_eq!(
            opening(request[..8].try_into().unwrap()),
            Opening::RateRequest
        );
        let body: [u8; RATE_REQUEST_BODY_LENGTH] = request[8..].try_into().unwrap();
        assert_eq!(decode_rate_request_body(body), Some(rate));
    }
}

#[test]
fn a_flipped_bit_is_caught() {
    let request = encode_rate_request(2_000_000);
    for byte in 8..RATE_REQUEST_LENGTH {
        for bit in 0..8 {
            let mut garbled = request;
            garbled[byte] ^= 1 << bit;
            let body: [u8; RATE_REQUEST_BODY_LENGTH] = garbled[8..].try_into().unwrap();
            assert_eq!(
                decode_rate_request_body(body),
                None,
                "byte {byte} bit {bit}"
            );
        }
    }
}

#[test]
fn a_size_is_a_size() {
    for size in [0_u64, 1, 0x80_0000, 0x3b40_0000, u64::from(u32::MAX)] {
        assert_eq!(opening(size.to_le_bytes()), Opening::Size(size));
    }
}

#[test]
fn the_magic_read_as_a_size_is_far_too_big() {
    // An older chainboot reads the magic as the size and refuses it as too big.
    let as_size = u64::from_le_bytes(RATE_REQUEST_MAGIC);
    assert!(as_size > 1 << 62);
}

#[test]
fn answers_are_told_apart() {
    assert_eq!(rate_answer("BAUD OK"), RateAnswer::Accepted);
    assert_eq!(rate_answer("BAUD OK and more"), RateAnswer::Accepted);
    assert_eq!(rate_answer("BAUD NO"), RateAnswer::Refused);
    assert_eq!(
        rate_answer("BAUD NO\r\n4000000 baud is out of range"),
        RateAnswer::Refused
    );
    // What a chainboot that does not know rate requests says.
    assert_eq!(rate_answer("ERR ❌ K"), RateAnswer::NotUnderstood);
    assert_eq!(rate_answer("BAUD"), RateAnswer::NotUnderstood);
    assert_eq!(rate_answer(""), RateAnswer::NotUnderstood);
}
