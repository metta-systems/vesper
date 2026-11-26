//! Host tests of the PSCI `CPU_ON` call and its error mapping against the Raspberry Pi 5
//! configuration.

#[cfg(test)]
mod tests {
    use {
        cpu_start_psci::{Firmware, Psci, cpu_on_status, psci_return},
        libcpu_start::{Conduit, CpuEntry, CpuStartConfig, StartStatus},
    };

    /// PSCI `CPU_ON`, SMC64: the standard function ID.
    const CPU_ON_64: u32 = 0xC400_0003;

    /// Records each call and answers with a fixed return code.
    struct Recorder {
        answer: i64,
        calls: Vec<(Conduit, u32, u64, u64, u64)>,
    }

    impl Recorder {
        fn answering(answer: i64) -> Self {
            Self {
                answer,
                calls: Vec::new(),
            }
        }
    }

    impl Firmware for Recorder {
        unsafe fn call(
            &mut self,
            conduit: Conduit,
            function: u32,
            x1: u64,
            x2: u64,
            x3: u64,
        ) -> i64 {
            self.calls.push((conduit, function, x1, x2, x3));
            self.answer
        }
    }

    /// The Raspberry Pi 5: Aff1 0..3, PSCI 1.0 over SMC.
    fn raspberry_pi_5() -> CpuStartConfig {
        let mut config = CpuStartConfig {
            cpu_count: 4,
            conduit: Conduit::Smc,
            psci_cpu_on: CPU_ON_64,
            ..CpuStartConfig::default()
        };
        for (index, cpu) in config.cpus.iter_mut().enumerate() {
            *cpu = CpuEntry {
                affinity: (index as u64) << 8,
                release_address: 0,
            };
        }
        config
    }

    const ENTRY: u64 = 0x8_0000;
    const CONTEXT: u64 = 0x10_0000;

    #[test]
    fn calls_cpu_on_with_affinity_entry_and_context() {
        let psci = Psci::new(&raspberry_pi_5()).expect("valid configuration");
        let mut firmware = Recorder::answering(psci_return::SUCCESS);
        assert_eq!(
            psci.start(3, 0x300, ENTRY, CONTEXT, &mut firmware),
            StartStatus::Success
        );
        assert_eq!(
            firmware.calls,
            [(Conduit::Smc, CPU_ON_64, 0x300, ENTRY, CONTEXT)]
        );
    }

    #[test]
    fn refuses_a_core_it_does_not_know_without_calling() {
        let psci = Psci::new(&raspberry_pi_5()).expect("valid configuration");
        let mut firmware = Recorder::answering(psci_return::SUCCESS);
        // Past the table, and the Aff0 numbering of the older boards.
        assert_eq!(
            psci.start(4, 0x400, ENTRY, CONTEXT, &mut firmware),
            StartStatus::NoSuchCpu
        );
        assert_eq!(
            psci.start(1, 1, ENTRY, CONTEXT, &mut firmware),
            StartStatus::NoSuchCpu
        );
        assert!(firmware.calls.is_empty());
    }

    #[test]
    fn maps_every_psci_return_code() {
        let expected = [
            (psci_return::SUCCESS, StartStatus::Success),
            (psci_return::NOT_SUPPORTED, StartStatus::Failed),
            (psci_return::INVALID_PARAMETERS, StartStatus::NoSuchCpu),
            (psci_return::DENIED, StartStatus::Denied),
            (psci_return::ALREADY_ON, StartStatus::AlreadyOn),
            (psci_return::ON_PENDING, StartStatus::Pending),
            (psci_return::INTERNAL_FAILURE, StartStatus::Failed),
            (psci_return::NOT_PRESENT, StartStatus::Denied),
            (psci_return::DISABLED, StartStatus::Denied),
            (psci_return::INVALID_ADDRESS, StartStatus::InvalidAddress),
            (-10, StartStatus::Failed),
            (1, StartStatus::Failed),
        ];
        for (code, status) in expected {
            assert_eq!(cpu_on_status(code), status, "code {code}");
        }
        // And through a call.
        let psci = Psci::new(&raspberry_pi_5()).expect("valid configuration");
        let mut firmware = Recorder::answering(psci_return::ALREADY_ON);
        assert_eq!(
            psci.start(1, 0x100, ENTRY, CONTEXT, &mut firmware),
            StartStatus::AlreadyOn
        );
    }

    #[test]
    fn needs_a_conduit_a_function_and_cpus() {
        let mut no_conduit = raspberry_pi_5();
        no_conduit.conduit = Conduit::None;
        let mut no_function = raspberry_pi_5();
        no_function.psci_cpu_on = 0;
        let mut no_cpus = raspberry_pi_5();
        no_cpus.cpu_count = 0;
        for config in [no_conduit, no_function, no_cpus] {
            assert_eq!(Psci::new(&config), Err(StartStatus::InvalidConfig));
        }
    }
}
