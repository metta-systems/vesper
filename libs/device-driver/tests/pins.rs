//! Host tests of the pin set a `badged by pins;` key carries.

#[cfg(test)]
mod tests {
    use vesper_device_driver::PinSet;

    #[test]
    fn a_pin_set_round_trips_through_its_badge() {
        let panel = [23, 24, 25]
            .into_iter()
            .try_fold(PinSet::EMPTY, PinSet::with)
            .expect("pins of the block");
        let badge = panel.badge();
        assert_eq!(badge, (1 << 63) | (1 << 23) | (1 << 24) | (1 << 25));
        assert_eq!(PinSet::from_badge(badge), Some(panel));
        assert!(panel.contains(24));
        assert!(!panel.contains(8));
        // The BCM2711's last pin fits; bit 63 is the mark, not a pin.
        assert!(PinSet::EMPTY.with(57).is_some_and(|set| set.contains(57)));
        assert_eq!(PinSet::EMPTY.with(63), None);
        assert!(!panel.contains(63));
    }

    #[test]
    fn an_empty_set_is_still_a_badge_and_owns_nothing() {
        let badge = PinSet::EMPTY.badge();
        assert_ne!(badge, 0, "zero would be an unbadged key");
        let owned = PinSet::from_badge(badge).expect("a pin set");
        assert!((0..PinSet::MAX_PINS).all(|pin| !owned.contains(pin)));
    }

    #[test]
    fn other_badges_are_not_pin_sets() {
        // Zero is an unbadged key; an area base (0x3a00_0000 and up) has no
        // bit 63.
        assert_eq!(PinSet::from_badge(0), None);
        assert_eq!(PinSet::from_badge(0x3a00_1000), None);
    }
}
