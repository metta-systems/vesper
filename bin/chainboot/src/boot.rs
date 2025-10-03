core::arch::global_asm!(
    core::include_str!("boot.s"),
    CONST_BOOT_CORE_ID = const 0,
    CONST_CORE_ID_MASK = const 0b11,
);
