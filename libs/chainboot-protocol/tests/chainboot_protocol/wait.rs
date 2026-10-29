use {core::time::Duration, vesper_chainboot_protocol::*};

fn millis(count: u64) -> Duration {
    Duration::from_millis(count)
}

#[test]
fn chainboot_waits_at_the_boot_rate_for_ever() {
    assert!(!BOOT_RATE_WAIT.gives_up(Duration::ZERO));
    assert!(!BOOT_RATE_WAIT.gives_up(Duration::from_secs(3600)));
    assert_eq!(BOOT_RATE_WAIT.beacon_period, Duration::from_secs(5));
}

#[test]
fn chainboot_gives_up_on_a_new_rate_after_three_seconds() {
    assert!(!FAST_RATE_WAIT.gives_up(Duration::ZERO));
    assert!(!FAST_RATE_WAIT.gives_up(millis(2_999)));
    assert!(FAST_RATE_WAIT.gives_up(millis(3_000)));
    assert!(FAST_RATE_WAIT.gives_up(millis(60_000)));
}

/// When chainboot beacons at a new rate: before each beacon it asks whether it
/// gives up, then waits a beacon period.
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
fn chainboot_beacons_six_times_at_a_new_rate() {
    assert_eq!(
        beacons(FAST_RATE_WAIT),
        [0, 500, 1_000, 1_500, 2_000, 2_500].map(millis)
    );
}

#[test]
fn the_host_hears_several_beacons_and_is_back_before_chainboot() {
    let heard = beacons(FAST_RATE_WAIT)
        .into_iter()
        .filter(|beacon| *beacon + SWITCH_SETTLE < HOST_FAST_BEACON_TIMEOUT)
        .count();
    assert!(heard >= 3, "the host hears {heard} beacons");
    // The host is back at the boot rate a beacon period before chainboot is.
    let chainboot_gives_up = FAST_RATE_WAIT.give_up_after.unwrap();
    assert!(HOST_FAST_BEACON_TIMEOUT + FAST_RATE_WAIT.beacon_period <= chainboot_gives_up);
}
