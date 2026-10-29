//! Host tests of the root dispatcher's choice: the highest runnable priority
//! level first, round-robin within a level, no starvation guard.

#[cfg(test)]
mod tests {
    use root_dispatcher::Rounds;

    const NORMAL: u8 = 0;
    const HIGH: u8 = 1;

    /// Choose among `levels` (slot `s` at index `s - 1`; `None`: not
    /// runnable); the chosen slot.
    fn pick(rounds: &mut Rounds, levels: &[Option<u8>]) -> Option<usize> {
        rounds
            .choose(|slot| levels[slot - 1].map(|level| (level, slot * 10)))
            .map(|(slot, seen)| {
                assert_eq!(seen, slot * 10, "what the record said comes back");
                slot
            })
    }

    #[test]
    fn one_level_is_round_robin() {
        let levels = [Some(NORMAL), Some(NORMAL), None, Some(NORMAL)];
        let mut rounds = Rounds::new(levels.len());
        let order: Vec<_> = (0..6).filter_map(|_| pick(&mut rounds, &levels)).collect();
        assert_eq!(order, [1, 2, 4, 1, 2, 4]);
    }

    #[test]
    fn nothing_runnable_chooses_nothing() {
        let mut rounds = Rounds::new(3);
        assert_eq!(pick(&mut rounds, &[None, None, None]), None);
        assert_eq!(Rounds::new(0).choose(|_| Some((NORMAL, ()))), None);
    }

    #[test]
    fn a_runnable_high_thread_always_runs_first() {
        // The high Thread sits between two busy normal ones; whenever it is
        // runnable it is chosen, whichever normal Thread ran last.
        let mut rounds = Rounds::new(4);
        let busy = [Some(NORMAL), None, Some(NORMAL), Some(NORMAL)];
        let woken = [Some(NORMAL), Some(HIGH), Some(NORMAL), Some(NORMAL)];
        for _ in 0..5 {
            assert_ne!(pick(&mut rounds, &busy), Some(2));
            assert_eq!(pick(&mut rounds, &woken), Some(2));
        }
    }

    #[test]
    fn no_starvation_guard() {
        // A high Thread that never blocks keeps the normal ones off the CPU.
        let mut rounds = Rounds::new(3);
        let levels = [Some(NORMAL), Some(HIGH), Some(NORMAL)];
        for _ in 0..10 {
            assert_eq!(pick(&mut rounds, &levels), Some(2));
        }
    }

    #[test]
    fn levels_keep_their_own_turns() {
        // Two high Threads take turns; the normal ones' turn is not disturbed
        // by the high ones running in between.
        let mut rounds = Rounds::new(5);
        let all = [Some(NORMAL), Some(HIGH), Some(NORMAL), Some(HIGH), Some(NORMAL)];
        let normal_only = [Some(NORMAL), None, Some(NORMAL), None, Some(NORMAL)];
        assert_eq!(pick(&mut rounds, &normal_only), Some(1));
        assert_eq!(pick(&mut rounds, &all), Some(2));
        assert_eq!(pick(&mut rounds, &all), Some(4));
        assert_eq!(pick(&mut rounds, &all), Some(2));
        // The normal level goes on after slot 1, where it left off.
        assert_eq!(pick(&mut rounds, &normal_only), Some(3));
        assert_eq!(pick(&mut rounds, &normal_only), Some(5));
        assert_eq!(pick(&mut rounds, &normal_only), Some(1));
    }

    #[test]
    fn more_levels_order_by_value() {
        // Levels above high work without any change: the highest wins.
        let mut rounds = Rounds::new(3);
        assert_eq!(pick(&mut rounds, &[Some(HIGH), Some(7), Some(255)]), Some(3));
        assert_eq!(pick(&mut rounds, &[Some(HIGH), Some(7), None]), Some(2));
    }
}
