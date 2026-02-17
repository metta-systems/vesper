#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[path = "../../../tests/common/mod.rs"]
mod common;

#[test_case]
pub fn test_invalid_phys_addr() {
    use libaddress::{PhysAddr, PhysAddrNotValid};

    let result = PhysAddr::try_new(0xfafa_0123_3210_3210);
    if let Err(e) = result {
        assert_eq!(e, PhysAddrNotValid::new(0xfafa_0123_3210_3210));
    } else {
        assert!(false)
    }
}

/// Sanity of [Address] methods.
#[test_case]
fn address_type_method_sanity() {
    use libaddress::{Address, Virtual};

    const SIZE: u64 = 0x1_0000;

    let addr = Address::<Virtual>::new(SIZE + 100);

    assert_eq!(addr.align_down_page(&SIZE), SIZE.into());

    assert_eq!(addr.align_up_page(&SIZE), (SIZE * 2).into());

    assert!(!addr.is_page_aligned(&SIZE));

    assert_eq!(addr.offset_into_page(&SIZE), 100);
}
#[test_case]
pub fn test_align_up() {
    use libaddress::align::align_up;

    // align 1
    assert_eq!(align_up(0, 1), 0);
    assert_eq!(align_up(1234, 1), 1234);
    assert_eq!(align_up(0xffff_ffff_ffff_ffff, 1), 0xffff_ffff_ffff_ffff);
    // align 2
    assert_eq!(align_up(0, 2), 0);
    assert_eq!(align_up(1233, 2), 1234);
    assert_eq!(align_up(0xffff_ffff_ffff_fffe, 2), 0xffff_ffff_ffff_fffe);
    // address 0
    assert_eq!(align_up(0, 128), 0);
    assert_eq!(align_up(0, 1), 0);
    assert_eq!(align_up(0, 2), 0);
    assert_eq!(align_up(0, 0x8000_0000_0000_0000), 0);
}
