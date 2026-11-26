//! Host tests of the start-method ABI: the record layouts Kickstart and a component
//! share, the status codes and the export macro.

#[cfg(test)]
mod tests {
    use {
        core::mem::{offset_of, size_of},
        vesper_cpu_start::{
            ABI_VERSION, Conduit, CpuEntry, CpuStartConfig, CpuStartOps, MAX_CPUS, StartStatus,
            cpu_start_export,
        },
    };

    #[test]
    fn cpu_entry_is_two_words() {
        assert_eq!(size_of::<CpuEntry>(), 16);
        assert_eq!(offset_of!(CpuEntry, affinity), 0);
        assert_eq!(offset_of!(CpuEntry, release_address), 8);
    }

    #[test]
    fn config_header_precedes_the_cpu_table() {
        assert_eq!(size_of::<Conduit>(), 4);
        assert_eq!(offset_of!(CpuStartConfig, cpu_count), 0);
        assert_eq!(offset_of!(CpuStartConfig, conduit), 4);
        assert_eq!(offset_of!(CpuStartConfig, psci_cpu_on), 8);
        assert_eq!(offset_of!(CpuStartConfig, reserved), 12);
        assert_eq!(offset_of!(CpuStartConfig, cpus), 16);
        assert_eq!(size_of::<CpuStartConfig>(), 16 + MAX_CPUS * 16);
    }

    #[test]
    fn ops_table_is_version_then_two_functions() {
        assert_eq!(offset_of!(CpuStartOps, abi_version), 0);
        assert_eq!(offset_of!(CpuStartOps, init), 8);
        assert_eq!(offset_of!(CpuStartOps, start), 16);
        assert_eq!(size_of::<CpuStartOps>(), 24);
    }

    #[test]
    fn config_lists_only_its_valid_cpus() {
        let mut config = CpuStartConfig {
            cpu_count: 2,
            ..CpuStartConfig::default()
        };
        config.cpus[1].affinity = 0x100;
        assert_eq!(config.cpus().len(), 2);
        assert_eq!(config.cpu(1).map(|cpu| cpu.affinity), Some(0x100));
        assert_eq!(config.cpu(2), None);
        // A count past the table is clamped, never read beyond it.
        config.cpu_count = u32::MAX;
        assert_eq!(config.cpus().len(), MAX_CPUS);
    }

    #[test]
    fn statuses_round_trip_and_unknown_values_fail() {
        for status in [
            StartStatus::Success,
            StartStatus::InvalidConfig,
            StartStatus::NoSuchCpu,
            StartStatus::AlreadyOn,
            StartStatus::Pending,
            StartStatus::InvalidAddress,
            StartStatus::Denied,
            StartStatus::Failed,
        ] {
            assert_eq!(StartStatus::from_raw(status as i32), status);
        }
        assert_eq!(StartStatus::from_raw(1), StartStatus::Failed);
        assert_eq!(StartStatus::from_raw(-100), StartStatus::Failed);
    }

    unsafe extern "C" fn accept(_config: *const CpuStartConfig) -> i32 {
        StartStatus::Success as i32
    }

    unsafe extern "C" fn refuse(_cpu: u32, _mpidr: u64, _entry: u64, _context: u64) -> i32 {
        StartStatus::NoSuchCpu as i32
    }

    cpu_start_export!(
        CpuStartOps {
            abi_version: ABI_VERSION,
            reserved: 0,
            init: accept,
            start: refuse,
        },
        enable_method = ["spin-table", "psci"]
    );

    #[test]
    fn export_names_the_table_and_its_methods() {
        assert_eq!(CPU_START_OPS.abi_version, ABI_VERSION);
        assert_eq!(&CPU_START_ENABLE_METHOD, b"spin-table\0psci\0");
        // SAFETY: the stub reads nothing.
        let status = unsafe { (CPU_START_OPS.start)(0, 0, 0, 0) };
        assert_eq!(StartStatus::from_raw(status), StartStatus::NoSuchCpu);
    }
}
