use vesper_usb_protocol::{descriptor::ParseError, enumeration::Speed, hub::*};

/// A hub descriptor: five ports, 4 ms... power-on-to-good of 50 (x2 ms), 1 mA.
const HUB: [u8; 9] = [0x09, 0x29, 0x05, 0xe0, 0x00, 0x32, 0x01, 0x00, 0xff];

fn status(status: u16, change: u16) -> PortStatus {
    PortStatus { status, change }
}

const CONNECTED: u16 = 1 << 0;
const ENABLED: u16 = 1 << 1;
const IN_RESET: u16 = 1 << 4;
const POWER: u16 = 1 << 8;
const LOW_SPEED: u16 = 1 << 9;
const HIGH_SPEED: u16 = 1 << 10;
const OVER_CURRENT: u16 = 1 << 3;
const CONNECTION_CHANGED: u16 = 1 << 0;
const RESET_CHANGED: u16 = 1 << 4;

#[test]
fn a_hub_descriptor_gives_the_ports_and_power_timing() {
    let hub = HubDescriptor::parse(&HUB).unwrap();
    assert_eq!(hub.ports, 5);
    assert_eq!(hub.characteristics, 0x00e0);
    assert_eq!(
        hub.power_on_to_good_ms, 100,
        "bPwrOn2PwrGood counts 2 ms units"
    );
    assert_eq!(hub.controller_current_ma, 1);
    // Bytes after the descriptor (the port bitmaps) are not looked at.
    let mut longer = [0; 11];
    longer[..9].copy_from_slice(&HUB);
    assert_eq!(HubDescriptor::parse(&longer), Ok(hub));
}

#[test]
fn malformed_hub_descriptors_are_refused() {
    assert_eq!(HubDescriptor::parse(&HUB[..6]), Err(ParseError::Truncated));
    assert_eq!(HubDescriptor::parse(&[]), Err(ParseError::Truncated));
    let mut wrong_type = HUB;
    wrong_type[1] = 0x01;
    assert_eq!(
        HubDescriptor::parse(&wrong_type),
        Err(ParseError::BadLength)
    );
    let mut no_ports = HUB;
    no_ports[2] = 0;
    assert_eq!(HubDescriptor::parse(&no_ports), Err(ParseError::BadLength));
    let mut short_length = HUB;
    short_length[0] = 6;
    assert_eq!(
        HubDescriptor::parse(&short_length),
        Err(ParseError::BadLength)
    );
    // The descriptor says it is longer than the bytes given.
    let mut long_length = HUB;
    long_length[0] = 12;
    assert_eq!(
        HubDescriptor::parse(&long_length),
        Err(ParseError::Truncated)
    );
}

#[test]
fn class_requests_have_the_wire_form_the_specification_gives() {
    assert_eq!(
        get_hub_descriptor(9).to_bytes(),
        [0xa0, 0x06, 0x00, 0x29, 0x00, 0x00, 0x09, 0x00]
    );
    assert_eq!(
        get_port_status(3).to_bytes(),
        [0xa3, 0x00, 0x00, 0x00, 0x03, 0x00, 0x04, 0x00]
    );
    assert_eq!(
        set_port_feature(2, PORT_POWER).to_bytes(),
        [0x23, 0x03, 0x08, 0x00, 0x02, 0x00, 0x00, 0x00]
    );
    assert_eq!(
        set_port_feature(2, PORT_RESET).to_bytes(),
        [0x23, 0x03, 0x04, 0x00, 0x02, 0x00, 0x00, 0x00]
    );
    assert_eq!(
        clear_port_feature(4, C_PORT_CONNECTION).to_bytes(),
        [0x23, 0x01, 0x10, 0x00, 0x04, 0x00, 0x00, 0x00]
    );
    assert_eq!(C_PORT_RESET, 20);
}

#[test]
fn port_status_decodes_the_four_bytes() {
    let value = PortStatus::from_bytes([0x03, 0x05, 0x01, 0x00]);
    assert_eq!(value.status, 0x0503);
    assert_eq!(value.change, 0x0001);
    assert!(value.connected() && value.enabled() && value.powered());
    assert!(value.connection_changed() && !value.reset_changed());
    assert!(!value.in_reset() && !value.over_current());
}

#[test]
fn port_speed_follows_the_status_bits() {
    assert_eq!(status(CONNECTED, 0).speed(), Speed::Full);
    assert_eq!(status(CONNECTED | LOW_SPEED, 0).speed(), Speed::Low);
    assert_eq!(status(CONNECTED | HIGH_SPEED, 0).speed(), Speed::High);
}

/// Drive a port to its first reset: power on, then a stable connection.
fn port_at_reset() -> Port {
    let mut port = Port::new(100);
    assert_eq!(port.next(PortEvent::Start), PortAction::PowerOn);
    assert_eq!(
        port.next(PortEvent::Done),
        PortAction::Wait { milliseconds: 100 }
    );
    assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
    let present = status(POWER | CONNECTED | LOW_SPEED, CONNECTION_CHANGED);
    assert_eq!(
        port.next(PortEvent::Status(present)),
        PortAction::ClearChange {
            feature: C_PORT_CONNECTION
        }
    );
    assert_eq!(
        port.next(PortEvent::Done),
        PortAction::Wait {
            milliseconds: POLL_MS
        }
    );
    let steady = status(POWER | CONNECTED | LOW_SPEED, 0);
    // Stable for DEBOUNCE_MS in POLL_MS steps, then the reset.
    let mut polls = 0;
    loop {
        assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
        polls += 1;
        match port.next(PortEvent::Status(steady)) {
            PortAction::Wait { milliseconds } => assert_eq!(milliseconds, POLL_MS),
            PortAction::Reset => break,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(polls * POLL_MS, DEBOUNCE_MS);
    port
}

#[test]
fn a_port_that_gave_up_takes_the_next_device_after_the_unplugging() {
    let mut port = port_at_reset();
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    let resetting = status(POWER | CONNECTED | IN_RESET | LOW_SPEED, 0);
    port.next(PortEvent::Status(resetting));
    let done = status(POWER | CONNECTED | ENABLED | LOW_SPEED, RESET_CHANGED);
    port.next(PortEvent::Status(done));
    port.next(PortEvent::Done);
    assert_eq!(
        port.next(PortEvent::Delayed),
        PortAction::Enumerate { speed: Speed::Low }
    );
    assert_eq!(
        port.next(PortEvent::Enumerated { ok: false }),
        PortAction::Failed
    );
    // Still plugged in: look again later.
    assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
    let plugged = status(POWER | CONNECTED | LOW_SPEED, 0);
    assert_eq!(
        port.next(PortEvent::Status(plugged)),
        PortAction::Wait {
            milliseconds: POLL_MS
        }
    );
    // Unplugged: the change is cleared and the port is empty again.
    assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
    let unplugged = status(POWER, CONNECTION_CHANGED);
    assert_eq!(
        port.next(PortEvent::Status(unplugged)),
        PortAction::ClearChange {
            feature: C_PORT_CONNECTION
        }
    );
    assert!(!port.failed());
}

#[test]
fn only_resting_ports_wait_for_a_status_change() {
    let mut port = Port::new(0);
    assert!(!port.waits_for_change(), "unpowered");
    port.next(PortEvent::Start);
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    assert!(port.waits_for_change(), "powered and empty");
    let port = port_at_reset();
    assert!(!port.waits_for_change(), "a reset is timed");
}

#[test]
fn a_deferred_reset_is_asked_for_again_at_the_next_look() {
    let mut port = port_at_reset();
    assert!(
        port.holds_default_address(),
        "a port in reset answers at address 0"
    );
    port.defer_reset();
    assert!(!port.holds_default_address());
    // One poll step later the debounce is complete and the reset comes again.
    assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
    let steady = status(POWER | CONNECTED | LOW_SPEED, 0);
    assert_eq!(port.next(PortEvent::Status(steady)), PortAction::Reset);
    assert!(port.holds_default_address());
}

#[test]
fn only_a_port_between_reset_and_address_holds_the_default_address() {
    assert!(!Port::new(0).holds_default_address());
    let mut port_in_reset = port_at_reset();
    port_in_reset.next(PortEvent::Done);
    let resetting = status(POWER | CONNECTED | IN_RESET | LOW_SPEED, 0);
    port_in_reset.next(PortEvent::Delayed);
    port_in_reset.next(PortEvent::Status(resetting));
    assert!(port_in_reset.holds_default_address());
}

#[test]
fn a_connected_port_is_debounced_reset_and_enumerated() {
    let mut port = port_at_reset();
    assert_eq!(
        port.next(PortEvent::Done),
        PortAction::Wait {
            milliseconds: RESET_POLL_MS
        }
    );
    assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
    // Still in reset: look again.
    let resetting = status(POWER | CONNECTED | IN_RESET | LOW_SPEED, 0);
    assert_eq!(
        port.next(PortEvent::Status(resetting)),
        PortAction::Wait {
            milliseconds: RESET_POLL_MS
        }
    );
    assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
    let reset_done = status(POWER | CONNECTED | ENABLED | LOW_SPEED, RESET_CHANGED);
    assert_eq!(
        port.next(PortEvent::Status(reset_done)),
        PortAction::ClearChange {
            feature: C_PORT_RESET
        }
    );
    assert_eq!(
        port.next(PortEvent::Done),
        PortAction::Wait {
            milliseconds: RESET_RECOVERY_MS
        }
    );
    assert_eq!(
        port.next(PortEvent::Delayed),
        PortAction::Enumerate { speed: Speed::Low }
    );
    assert!(!port.has_device());
    assert_eq!(
        port.next(PortEvent::Enumerated { ok: true }),
        PortAction::Wait {
            milliseconds: POLL_MS
        }
    );
    assert!(port.has_device());
}

fn enumerated_port() -> Port {
    let mut port = port_at_reset();
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    let reset_done = status(POWER | CONNECTED | ENABLED | HIGH_SPEED, RESET_CHANGED);
    port.next(PortEvent::Status(reset_done));
    port.next(PortEvent::Done);
    assert_eq!(
        port.next(PortEvent::Delayed),
        PortAction::Enumerate { speed: Speed::High }
    );
    port.next(PortEvent::Enumerated { ok: true });
    port
}

#[test]
fn an_enumerated_port_is_watched_and_torn_down_when_the_device_goes() {
    let mut port = enumerated_port();
    let steady = status(POWER | CONNECTED | ENABLED | HIGH_SPEED, 0);
    for _ in 0..3 {
        assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
        assert_eq!(
            port.next(PortEvent::Status(steady)),
            PortAction::Wait {
                milliseconds: POLL_MS
            }
        );
    }
    assert!(port.has_device());
    // Unplugged.
    assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
    let gone = status(POWER, CONNECTION_CHANGED);
    assert_eq!(port.next(PortEvent::Status(gone)), PortAction::Teardown);
    assert!(!port.has_device());
    assert_eq!(
        port.next(PortEvent::Done),
        PortAction::ClearChange {
            feature: C_PORT_CONNECTION
        }
    );
    // The port is empty again and watches for the next device.
    assert_eq!(
        port.next(PortEvent::Done),
        PortAction::Wait {
            milliseconds: POLL_MS
        }
    );
    assert_eq!(port.next(PortEvent::Delayed), PortAction::PollStatus);
}

#[test]
fn a_replug_between_polls_tears_the_device_down() {
    let mut port = enumerated_port();
    port.next(PortEvent::Delayed);
    // Still connected, but the change bit says it went away and came back.
    let replugged = status(POWER | CONNECTED | ENABLED, CONNECTION_CHANGED);
    assert_eq!(
        port.next(PortEvent::Status(replugged)),
        PortAction::Teardown
    );
}

#[test]
fn a_connection_that_bounces_starts_the_debounce_over() {
    let mut port = Port::new(0);
    port.next(PortEvent::Start);
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    let steady = status(POWER | CONNECTED, 0);
    port.next(PortEvent::Status(steady));
    // Two quiet polls, then a bounce.
    for _ in 0..2 {
        port.next(PortEvent::Delayed);
        port.next(PortEvent::Status(steady));
    }
    port.next(PortEvent::Delayed);
    let bounced = status(POWER | CONNECTED, CONNECTION_CHANGED);
    assert_eq!(
        port.next(PortEvent::Status(bounced)),
        PortAction::ClearChange {
            feature: C_PORT_CONNECTION
        }
    );
    // The full debounce is needed again.
    port.next(PortEvent::Done);
    let mut polls = 0;
    loop {
        port.next(PortEvent::Delayed);
        polls += 1;
        if port.next(PortEvent::Status(steady)) == PortAction::Reset {
            break;
        }
    }
    assert_eq!(polls * POLL_MS, DEBOUNCE_MS);
}

#[test]
fn a_device_unplugged_while_debouncing_or_resetting_is_forgotten() {
    // While debouncing.
    let mut port = Port::new(0);
    port.next(PortEvent::Start);
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    port.next(PortEvent::Status(status(POWER | CONNECTED, 0)));
    port.next(PortEvent::Delayed);
    assert_eq!(
        port.next(PortEvent::Status(status(POWER, 0))),
        PortAction::Wait {
            milliseconds: POLL_MS
        }
    );
    assert!(!port.has_device() && !port.failed());
    // While resetting.
    let mut port = port_at_reset();
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    assert_eq!(
        port.next(PortEvent::Status(status(POWER, CONNECTION_CHANGED))),
        PortAction::Wait {
            milliseconds: POLL_MS
        }
    );
    assert!(!port.failed());
}

#[test]
fn over_current_and_a_reset_that_leaves_the_port_disabled_fail_the_port() {
    let mut port = Port::new(0);
    port.next(PortEvent::Start);
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    assert_eq!(
        port.next(PortEvent::Status(status(POWER | OVER_CURRENT, 0))),
        PortAction::Failed
    );
    assert!(port.failed());
    assert_eq!(port.next(PortEvent::Delayed), PortAction::Idle);

    let mut port = port_at_reset();
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    let disabled = status(POWER | CONNECTED, RESET_CHANGED);
    assert_eq!(port.next(PortEvent::Status(disabled)), PortAction::Failed);
    assert!(port.failed());

    // A device that fails enumeration fails the port.
    let mut port = port_at_reset();
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    port.next(PortEvent::Status(status(
        POWER | CONNECTED | ENABLED,
        RESET_CHANGED,
    )));
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    assert_eq!(
        port.next(PortEvent::Enumerated { ok: false }),
        PortAction::Failed
    );
    assert!(port.failed());
}

#[test]
fn events_that_do_not_fit_the_state_are_ignored() {
    let mut port = Port::new(0);
    assert_eq!(port.next(PortEvent::Delayed), PortAction::Idle);
    assert_eq!(
        port.next(PortEvent::Enumerated { ok: true }),
        PortAction::Idle
    );
}

#[test]
fn a_port_is_settled_once_it_rests_empty_enumerated_or_failed() {
    // Not before power-on, not while waiting for the first look.
    let mut port = Port::new(0);
    assert!(!port.settled());
    port.next(PortEvent::Start);
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    assert!(!port.settled(), "no look at the port yet");
    // An empty port that was looked at.
    port.next(PortEvent::Status(status(POWER, 0)));
    assert!(port.settled());
    // A connection unsettles it until the device is enumerated.
    port.next(PortEvent::Delayed);
    port.next(PortEvent::Status(status(POWER | CONNECTED, 0)));
    assert!(!port.settled());

    let port = enumerated_port();
    assert!(port.settled());

    // An empty port with a change bit to clear is not quiet yet.
    let mut port = Port::new(0);
    port.next(PortEvent::Start);
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    port.next(PortEvent::Status(status(POWER, CONNECTION_CHANGED)));
    assert!(!port.settled());
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    port.next(PortEvent::Status(status(POWER, 0)));
    assert!(port.settled());

    // A failed port is settled too.
    let mut port = Port::new(0);
    port.next(PortEvent::Start);
    port.next(PortEvent::Done);
    port.next(PortEvent::Delayed);
    port.next(PortEvent::Status(status(POWER | OVER_CURRENT, 0)));
    assert!(port.settled());
}
