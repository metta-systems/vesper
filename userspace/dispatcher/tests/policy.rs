//! Host tests of the root dispatcher's choice: the highest runnable priority
//! level first, round-robin within a level, and the starvation guard.

#[cfg(test)]
mod tests {
    use root_dispatcher::Rounds;

    // The levels of `libdevice_driver::Priority`.
    const NORMAL: u8 = 0;
    const INPUT: u8 = 1;
    const HIGH: u8 = 2;

    /// The dispatcher's slice.
    const SLICE_NS: u64 = 10_000_000;

    fn rounds(slot_count: usize) -> Rounds {
        Rounds::new(slot_count, SLICE_NS)
    }

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

    /// Execution clocks of the slots, as the records would show them.
    struct Clocks([u64; 8]);

    impl Clocks {
        /// Pick among `levels`, then let the chosen slot run `ran_ns` and
        /// stop, blocked or not, and report it; the chosen slot.
        fn run(
            &mut self,
            rounds: &mut Rounds,
            levels: &[Option<u8>],
            ran_ns: u64,
            blocked: bool,
        ) -> Option<usize> {
            let slot = pick(rounds, levels)?;
            self.0[slot] += ran_ns;
            let level = levels[slot - 1].expect("a chosen slot is runnable");
            rounds.ran(slot, level, self.0[slot], blocked);
            Some(slot)
        }
    }

    #[test]
    fn one_level_is_round_robin() {
        let levels = [Some(NORMAL), Some(NORMAL), None, Some(NORMAL)];
        let mut rounds = rounds(levels.len());
        let order: Vec<_> = (0..6).filter_map(|_| pick(&mut rounds, &levels)).collect();
        assert_eq!(order, [1, 2, 4, 1, 2, 4]);
    }

    #[test]
    fn nothing_runnable_chooses_nothing() {
        let mut rounds = rounds(3);
        assert_eq!(pick(&mut rounds, &[None, None, None]), None);
        assert_eq!(
            Rounds::new(0, SLICE_NS).choose(|_| Some((NORMAL, ()))),
            None
        );
    }

    #[test]
    fn a_runnable_high_thread_always_runs_first() {
        // The high Thread sits between two busy normal ones; whenever it is
        // runnable it is chosen, whichever normal Thread ran last.
        let mut rounds = rounds(4);
        let busy = [Some(NORMAL), None, Some(NORMAL), Some(NORMAL)];
        let woken = [Some(NORMAL), Some(HIGH), Some(NORMAL), Some(NORMAL)];
        for _ in 0..5 {
            assert_ne!(pick(&mut rounds, &busy), Some(2));
            assert_eq!(pick(&mut rounds, &woken), Some(2));
        }
    }

    #[test]
    fn a_spinning_input_thread_lets_normal_threads_run() {
        // The input Thread never blocks. After one slice of it the normal
        // Threads take turns with it.
        let mut rounds = rounds(3);
        let mut clocks = Clocks([0; 8]);
        let levels = [Some(NORMAL), Some(INPUT), Some(NORMAL)];
        assert_eq!(clocks.run(&mut rounds, &levels, SLICE_NS, false), Some(2));
        assert!(rounds.demoted(2));
        assert_eq!(rounds.demotions(), 1);
        let order: Vec<_> = (0..6)
            .filter_map(|_| clocks.run(&mut rounds, &levels, SLICE_NS, false))
            .collect();
        assert!(order.contains(&1) && order.contains(&3), "{order:?}");
        // Round-robin at the normal level: every Thread gets every third turn.
        assert_eq!(order.iter().filter(|&&slot| slot == 2).count(), 2);
        assert_eq!(rounds.demotions(), 1, "a demoted Thread is counted once");
    }

    #[test]
    fn preemptions_add_up_to_a_slice() {
        // Woken Threads keep cutting the input Thread's runs short; the runs
        // still add up, and it is demoted after a slice of them.
        let mut rounds = rounds(2);
        let mut clocks = Clocks([0; 8]);
        let levels = [Some(NORMAL), Some(INPUT)];
        for _ in 0..4 {
            assert_eq!(
                clocks.run(&mut rounds, &levels, SLICE_NS / 4 - 1, false),
                Some(2)
            );
        }
        assert!(!rounds.demoted(2));
        assert_eq!(clocks.run(&mut rounds, &levels, 4, false), Some(2));
        assert!(rounds.demoted(2));
        assert_eq!(pick(&mut rounds, &levels), Some(1));
    }

    #[test]
    fn a_demoted_thread_regains_its_level_once_it_blocks() {
        let mut rounds = rounds(2);
        let mut clocks = Clocks([0; 8]);
        let levels = [Some(NORMAL), Some(INPUT)];
        clocks.run(&mut rounds, &levels, 2 * SLICE_NS, false);
        assert!(rounds.demoted(2));
        // Its turn at the normal level comes after slot 1's.
        assert_eq!(clocks.run(&mut rounds, &levels, SLICE_NS, false), Some(1));
        assert_eq!(clocks.run(&mut rounds, &levels, 1_000, true), Some(2));
        assert!(!rounds.demoted(2));
        for _ in 0..3 {
            assert_eq!(pick(&mut rounds, &levels), Some(2));
        }
    }

    #[test]
    fn blocking_input_and_high_threads_keep_precedence() {
        // Raised Threads that run a little and block, again and again, are
        // never demoted, however much they run in total.
        let mut rounds = rounds(3);
        let mut clocks = Clocks([0; 8]);
        let levels = [Some(NORMAL), Some(INPUT), Some(HIGH)];
        for _ in 0..100 {
            assert_eq!(
                clocks.run(&mut rounds, &levels, SLICE_NS / 2, true),
                Some(3)
            );
            assert_eq!(
                clocks.run(
                    &mut rounds,
                    &[Some(NORMAL), Some(INPUT), None],
                    SLICE_NS / 2,
                    true
                ),
                Some(2)
            );
        }
        assert!(!rounds.demoted(2) && !rounds.demoted(3));
        assert_eq!(rounds.demotions(), 0);
    }

    #[test]
    fn high_preempts_a_spinning_input_thread() {
        // A demoted input Thread runs among the normal ones; a woken
        // high Thread still runs first.
        let mut rounds = rounds(3);
        let mut clocks = Clocks([0; 8]);
        let spinning = [Some(NORMAL), Some(INPUT), None];
        let woken = [Some(NORMAL), Some(INPUT), Some(HIGH)];
        clocks.run(&mut rounds, &spinning, SLICE_NS, false);
        assert!(rounds.demoted(2));
        for _ in 0..5 {
            assert_eq!(clocks.run(&mut rounds, &woken, 100_000, true), Some(3));
            clocks.run(&mut rounds, &spinning, SLICE_NS, false);
        }
    }

    #[test]
    fn a_new_thread_in_a_slot_starts_from_zero() {
        // A total below the last one is a fresh Thread: what it ran counts,
        // not the difference.
        let mut rounds = rounds(1);
        rounds.ran(1, INPUT, 50 * SLICE_NS, true);
        rounds.ran(1, INPUT, SLICE_NS / 2, false);
        assert!(!rounds.demoted(1));
        rounds.ran(1, INPUT, SLICE_NS, false);
        assert!(rounds.demoted(1));
    }

    #[test]
    fn a_normal_thread_is_never_demoted_below_normal() {
        let mut rounds = rounds(2);
        let mut clocks = Clocks([0; 8]);
        let levels = [Some(NORMAL), Some(NORMAL)];
        let order: Vec<_> = (0..4)
            .filter_map(|_| clocks.run(&mut rounds, &levels, SLICE_NS, false))
            .collect();
        assert_eq!(order, [1, 2, 1, 2]);
        assert_eq!(rounds.demotions(), 0);
    }

    #[test]
    fn levels_keep_their_own_turns() {
        // Two high Threads take turns; the normal ones' turn is not disturbed
        // by the high ones running in between.
        let mut rounds = rounds(5);
        let all = [
            Some(NORMAL),
            Some(HIGH),
            Some(NORMAL),
            Some(HIGH),
            Some(NORMAL),
        ];
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
    fn input_runs_between_normal_and_high() {
        // A woken input Thread runs ahead of the busy normal ones, and the
        // playback Thread ahead of it; with the playback Thread blocked, the
        // input Threads take turns ahead of the normal ones.
        let mut rounds = rounds(5);
        let all = [
            Some(NORMAL),
            Some(INPUT),
            Some(HIGH),
            Some(INPUT),
            Some(NORMAL),
        ];
        assert_eq!(pick(&mut rounds, &all), Some(3));
        let without_high = [Some(NORMAL), Some(INPUT), None, Some(INPUT), Some(NORMAL)];
        assert_eq!(pick(&mut rounds, &without_high), Some(2));
        assert_eq!(pick(&mut rounds, &without_high), Some(4));
        assert_eq!(pick(&mut rounds, &without_high), Some(2));
        let normal_only = [Some(NORMAL), None, None, None, Some(NORMAL)];
        assert_eq!(pick(&mut rounds, &normal_only), Some(1));
        assert_eq!(pick(&mut rounds, &normal_only), Some(5));
    }

    #[test]
    fn more_levels_order_by_value() {
        // Levels above high work without any change: the highest wins.
        let mut rounds = rounds(3);
        assert_eq!(
            pick(&mut rounds, &[Some(HIGH), Some(7), Some(255)]),
            Some(3)
        );
        assert_eq!(pick(&mut rounds, &[Some(HIGH), Some(7), None]), Some(2));
    }
}
