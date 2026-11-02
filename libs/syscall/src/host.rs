//! The syscall wrappers' signatures for hosts that are not aarch64.
//!
//! Host-tested crates still compile their syscall callers; any call reaching
//! one of these is a test that forgot to substitute its transport.

const UNAVAILABLE: &str = "Vesper syscalls are only available on aarch64";

/// See the aarch64 `protected_call6`.
///
/// # Safety
/// - Never returns: panics.
#[allow(clippy::too_many_arguments)]
pub unsafe fn protected_call6(
    _cap: u64,
    _op: u64,
    _a0: u64,
    _a1: u64,
    _a2: u64,
    _a3: u64,
    _a4: u64,
    _a5: u64,
) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `protected_call5`.
///
/// # Safety
/// - Never returns: panics.
pub unsafe fn protected_call5(
    _cap: u64,
    _op: u64,
    _a0: u64,
    _a1: u64,
    _a2: u64,
    _a3: u64,
    _a4: u64,
) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `protected_call4`.
///
/// # Safety
/// - Never returns: panics.
pub unsafe fn protected_call4(
    _cap: u64,
    _op: u64,
    _a0: u64,
    _a1: u64,
    _a2: u64,
    _a3: u64,
) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `protected_call3`.
///
/// # Safety
/// - Never returns: panics.
pub unsafe fn protected_call3(
    _cap: u64,
    _op: u64,
    _a0: u64,
    _a1: u64,
    _a2: u64,
) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `protected_call2`.
///
/// # Safety
/// - Never returns: panics.
pub unsafe fn protected_call2(_cap: u64, _op: u64, _a0: u64, _a1: u64) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `protected_call1`.
///
/// # Safety
/// - Never returns: panics.
pub unsafe fn protected_call1(_cap: u64, _op: u64, _a0: u64) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `protected_call0`.
///
/// # Safety
/// - Never returns: panics.
pub unsafe fn protected_call0(_cap: u64, _op: u64) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `ppc_call`.
pub fn ppc_call(_key: u64, _args: [u64; 6]) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `ppc_call_with_key`.
pub fn ppc_call_with_key(_key: u64, _lent_key: u64, _args: [u64; 6]) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `protected_call6_with_x8`.
///
/// # Safety
/// - Never returns: panics.
#[allow(clippy::too_many_arguments)]
pub unsafe fn protected_call6_with_x8(
    _cap: u64,
    _op: u64,
    _a0: u64,
    _a1: u64,
    _a2: u64,
    _a3: u64,
    _a4: u64,
    _a5: u64,
    _x8: u64,
) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}

/// See the aarch64 `ppc_return`.
///
/// # Safety
/// - Never returns: panics.
pub unsafe fn ppc_return(_key: u64, _r0: u64, _r1: u64) -> (u64, u64, u64) {
    unimplemented!("{UNAVAILABLE}")
}
