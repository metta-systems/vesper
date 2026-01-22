#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

mod common;

#[test_case]
pub fn write_to_works() {
    let mut buf = [0u8; 64];
    let s: &str = libconsole::write_to::show(
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
    let s: &str = libconsole::write_to::c_show(
        &mut buf,
        format_args!("write some stuff {:?}: {}", "foo", 42),
    )
    .unwrap();
    assert_eq!(s, "write some stuff \"foo\": 42\0");
    assert_eq!(s.as_ptr(), buf.as_ptr());
}
