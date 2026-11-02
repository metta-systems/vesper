#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[path = "../../../tests/common/mod.rs"]
mod common;

#[test_case]
pub fn write_to_works() {
    let mut buf = [0u8; 64];
    let s = vesper_print::format_str(
        &mut buf,
        format_args!("write some stuff {:?}: {}", "foo", 42),
    )
    .unwrap();
    assert_eq!(s, "write some stuff \"foo\": 42");
    assert_eq!(s.as_ptr(), buf.as_ptr());
}

#[test_case]
pub fn zero_terminated_write_to_works() {
    let mut buf = [0u8; 64];
    let s = vesper_print::format_cstr(
        &mut buf,
        format_args!("write some stuff {:?}: {}", "foo", 42),
    )
    .unwrap();
    assert_eq!(s.to_bytes_with_nul(), b"write some stuff \"foo\": 42\0");
    assert_eq!(s.to_bytes_with_nul().as_ptr(), buf.as_ptr());
}
