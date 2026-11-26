# SMP completed

One line per finished item of [smp-implementation-plan.md](smp-implementation-plan.md).

## SMP0: decisions

- [x] Multicore rules in [smp-design.md](smp-design.md#decisions); the contract's serialized-kernel foundation and the capabilities design's CONFIRMED note link there.

## SMP1: cores online, idle

- [x] Core identity by full affinity: `libcpu::smp::CoreAffinity` (Aff3, Aff2..Aff0 from the typed `MPIDR_EL1` fields, PSCI `CPU_ON` layout) replaces `core_id()`'s `MPIDR & 0x3`; `libboot`'s `_boot_cores` and chainboot's `_start` compare the full affinity with `libcpu::smp::BOOT_CORE_AFFINITY`. Host tests `just _host-test vesper-cpu smp` (Pi 3 Aff0, Pi 5 Aff1 numbering, Aff3); booted under QEMU by `_test-memory`, `_test-irq-rpi4` and `_test-chainboot`.
