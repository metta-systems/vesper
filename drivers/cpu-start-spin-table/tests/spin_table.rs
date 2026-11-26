//! Host tests of the spin-table release logic against the Raspberry Pi 3 and 4 table.

#[cfg(test)]
mod tests {
    use {
        cpu_start_spin_table::{ReleaseWord, SpinTable},
        libcpu_start::{CpuEntry, CpuStartConfig, StartStatus},
    };

    /// Records every release instead of touching memory.
    #[derive(Default)]
    struct Recorder {
        releases: Vec<(u64, u64)>,
    }

    impl ReleaseWord for Recorder {
        unsafe fn release(&mut self, word: u64, entry: u64) {
            self.releases.push((word, entry));
        }
    }

    /// The Raspberry Pi 3 and 4: Aff0 0..3, release words 0xd8, 0xe0, 0xe8, 0xf0.
    fn raspberry_pi() -> CpuStartConfig {
        let mut config = CpuStartConfig {
            cpu_count: 4,
            ..CpuStartConfig::default()
        };
        for (index, word) in [0xd8, 0xe0, 0xe8, 0xf0].into_iter().enumerate() {
            config.cpus[index] = CpuEntry {
                affinity: index as u64,
                release_address: word,
            };
        }
        config
    }

    const ENTRY: u64 = 0x8_0000;

    #[test]
    fn releases_each_core_through_its_own_word() {
        let table = SpinTable::new(&raspberry_pi()).expect("valid table");
        let mut machine = Recorder::default();
        for cpu in 1..4 {
            assert_eq!(
                table.start(cpu, u64::from(cpu), ENTRY, &mut machine),
                StartStatus::Success
            );
        }
        assert_eq!(
            machine.releases,
            [(0xe0, ENTRY), (0xe8, ENTRY), (0xf0, ENTRY)]
        );
    }

    #[test]
    fn refuses_a_core_it_does_not_know_and_writes_nothing() {
        let table = SpinTable::new(&raspberry_pi()).expect("valid table");
        let mut machine = Recorder::default();
        // Past the table, and an affinity that does not match the index.
        assert_eq!(
            table.start(4, 4, ENTRY, &mut machine),
            StartStatus::NoSuchCpu
        );
        assert_eq!(
            table.start(2, 0x200, ENTRY, &mut machine),
            StartStatus::NoSuchCpu
        );
        assert!(machine.releases.is_empty());
    }

    #[test]
    fn refuses_an_entry_that_is_no_instruction_address() {
        let table = SpinTable::new(&raspberry_pi()).expect("valid table");
        let mut machine = Recorder::default();
        for entry in [0, ENTRY + 2] {
            assert_eq!(
                table.start(1, 1, entry, &mut machine),
                StartStatus::InvalidAddress
            );
        }
        assert!(machine.releases.is_empty());
    }

    #[test]
    fn needs_cpus_with_release_words() {
        assert_eq!(
            SpinTable::new(&CpuStartConfig::default()),
            Err(StartStatus::InvalidConfig)
        );
        let mut missing = raspberry_pi();
        missing.cpus[3].release_address = 0;
        assert_eq!(SpinTable::new(&missing), Err(StartStatus::InvalidConfig));
    }
}
