use {
    crate::{exception::PrivilegeLevel, info},
    aarch64_cpu::registers::*,
    core::cell::UnsafeCell,
    tock_registers::interfaces::Readable,
};

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// The processor's current privilege level.
pub fn current_privilege_level() -> (PrivilegeLevel, &'static str) {
    let el = CurrentEL.read_as_enum(CurrentEL::EL);
    match el {
        Some(CurrentEL::EL::Value::EL3) => (PrivilegeLevel::Unknown, "EL3"),
        Some(CurrentEL::EL::Value::EL2) => (PrivilegeLevel::Hypervisor, "EL2"),
        Some(CurrentEL::EL::Value::EL1) => (PrivilegeLevel::Kernel, "EL1"),
        Some(CurrentEL::EL::Value::EL0) => (PrivilegeLevel::User, "EL0"),
        _ => (PrivilegeLevel::Unknown, "Unknown"),
    }
}

pub fn handling_init() {
    extern "Rust" {
        static __EXCEPTION_VECTORS_START: UnsafeCell<()>;
    }

    unsafe {
        super::traps::set_vbar_el1_checked(__EXCEPTION_VECTORS_START.get() as u64)
            .expect("Vector table properly aligned!");
    }
    info!("[!] Exception traps set up");
}
