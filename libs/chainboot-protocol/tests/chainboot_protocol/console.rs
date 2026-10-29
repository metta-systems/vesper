use {core::time::Duration, vesper_chainboot_protocol::*};

fn millis(count: u64) -> Duration {
    Duration::from_millis(count)
}

#[test]
fn the_handoff_is_these_registers() {
    // `BAUDKEEP` read as a little-endian u64.
    assert_eq!(HANDOFF_MAGIC, 0x5045_454b_4455_4142);
    assert_eq!(
        handoff_registers(1_500_000),
        [0x5045_454b_4455_4142, 1_500_000]
    );
}

#[test]
fn a_handoff_keeps_its_rate() {
    for rate in [115_200, 1_500_000, 2_000_000, u32::MAX] {
        assert_eq!(kept_rate(handoff_registers(rate)), Some(rate));
    }
}

#[test]
fn what_the_firmware_leaves_is_no_handoff() {
    // The firmware's stub and QEMU's boot code zero x1 and x2.
    assert_eq!(kept_rate([0, 0]), None);
    // The magic with no rate, or a rate that is not one.
    assert_eq!(kept_rate([HANDOFF_MAGIC, 0]), None);
    assert_eq!(kept_rate([HANDOFF_MAGIC, 1 << 32]), None);
    // A rate without the magic.
    assert_eq!(kept_rate([0, 115_200]), None);
    assert_eq!(kept_rate([HANDOFF_MAGIC ^ 1, 115_200]), None);
}

#[test]
fn the_console_beacon_and_ack_are_these_bytes() {
    assert_eq!(CONSOLE_BEACON, [0x02, 0x02, 0x02]);
    assert_eq!(CONSOLE_ACK, [0x06, 0x06, 0x06]);
}

/// What a chainofcommand that knows only the load beacon does with what
/// arrives: it counts `^C` in a row, and starts an upload at three.
fn an_older_host_starts_an_upload(arriving: &[u8]) -> bool {
    let mut breaks = 0;
    for byte in arriving {
        if *byte == 3 {
            breaks += 1;
            if breaks == 3 {
                return true;
            }
        } else {
            breaks = 0;
        }
    }
    false
}

#[test]
fn an_older_host_ignores_the_console_beacon() {
    let mut window = Vec::new();
    for _ in 0..6 {
        window.extend_from_slice(&CONSOLE_BEACON);
    }
    window.extend_from_slice(b"\r\n\xf0\x9f\xa5\xbe Console at 115200 baud\r\n");
    assert!(!an_older_host_starts_an_upload(&window));
    // The load beacon still starts one.
    assert!(an_older_host_starts_an_upload(&BEACON));
}

#[test]
fn a_run_is_its_byte_length_times_in_a_row() {
    let mut beacon = Run::of(CONSOLE_BEACON);
    assert!(!beacon.push(2));
    assert!(!beacon.push(2));
    assert!(beacon.push(2));
    // It starts over after a run, and at any other byte.
    assert!(!beacon.push(2));
    assert!(!beacon.push(2));
    assert!(!beacon.push(b'x'));
    assert!(!beacon.push(2));
    assert!(!beacon.push(2));
    assert!(beacon.push(2));

    let mut upload = Run::of(BEACON);
    assert!(!CONSOLE_BEACON.iter().any(|byte| upload.push(*byte)));
    assert!(BEACON.iter().any(|byte| upload.push(*byte)));
}

#[test]
fn an_ack_split_by_noise_is_no_ack() {
    let mut ack = Run::of(CONSOLE_ACK);
    assert!(![6, 6, 0, 6, 6].iter().any(|byte| ack.push(*byte)));
    assert!(ack.push(6));
}

/// When a beacon goes out under `wait`, after the start of the wait.
fn beacons(wait: Wait) -> Vec<Duration> {
    let mut sent = Vec::new();
    let mut waited = Duration::ZERO;
    while !wait.gives_up(waited) {
        sent.push(waited);
        waited += wait.beacon_period;
    }
    sent
}

#[test]
fn the_console_is_offered_for_a_second_and_a_half() {
    assert_eq!(
        beacons(CONSOLE_WAIT),
        [0, 250, 500, 750, 1_000, 1_250].map(millis)
    );
    assert_eq!(CONSOLE_WAIT.give_up_after, Some(millis(1_500)));
}

#[test]
fn the_host_hears_several_beacons_at_the_new_rate_and_gives_up_first() {
    // The kernel beacons from SWITCH_SETTLE after it switched.
    let sent: Vec<_> = beacons(CONSOLE_CONFIRM_WAIT)
        .into_iter()
        .map(|beacon| beacon + SWITCH_SETTLE)
        .collect();
    assert_eq!(sent.len(), 10);
    let heard = sent
        .iter()
        .filter(|beacon| **beacon < HOST_CONSOLE_CONFIRM_TIMEOUT)
        .count();
    assert!(heard >= 5, "the host hears {heard} beacons");
    // The kernel listens until SWITCH_SETTLE and the limit after switching; the
    // host's last ACK leaves a beacon period before that.
    let kernel_listens_until = SWITCH_SETTLE + CONSOLE_CONFIRM_WAIT.give_up_after.unwrap();
    assert_eq!(kernel_listens_until, millis(1_100));
    assert!(
        HOST_CONSOLE_CONFIRM_TIMEOUT + CONSOLE_CONFIRM_WAIT.beacon_period <= kernel_listens_until
    );
}

#[test]
fn a_whole_rate_request_fits_in_the_gap() {
    // Thirteen ten-bit frames at the boot rate.
    let frame_micros = 10 * 1_000_000 / u64::from(BOOT_BAUD_RATE);
    let request = Duration::from_micros(frame_micros * RATE_REQUEST_LENGTH as u64);
    assert!(request < millis(2));
    assert!(CONSOLE_REQUEST_GAP < CONSOLE_WAIT.beacon_period);
}
