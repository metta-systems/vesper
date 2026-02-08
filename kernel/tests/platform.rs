#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

use {
    libaddress::{Address, Virtual},
    libplatform::platform::device_driver::{
        mailbox::{self, tag, Mailbox},
        Function, RateDivisors, GPIO,
    },
};

mod common;

#[test_case]
fn test_pin_transitions() {
    let mut reg = [0u32; 40];
    let mmio_base_addr = Address::<Virtual>::new(&mut reg as *mut _ as usize);
    let gpio = unsafe { GPIO::new(mmio_base_addr) };

    let _out = gpio.get_pin(1).into_output();
    assert_eq!(reg[0], 0b001_000);
    let _inp = gpio.get_pin(12).into_input();
    assert_eq!(reg[1], 0b000_000_000);
    let _alt = gpio.get_pin(35).into_alt(Function::Alt1);
    assert_eq!(reg[3], 0b101_000_000_000_000_000);
}

#[test_case]
fn test_pin_outputs() {
    let mut reg = [0u32; 40];
    let mmio_base_addr = Address::<Virtual>::new(&mut reg as *mut _ as usize);
    let gpio = unsafe { GPIO::new(mmio_base_addr) };

    let pin = gpio.get_pin(1);
    let mut out = pin.into_output();
    out.set();
    assert_eq!(reg[7], 0b10); // SET pin 1 = 1 << 1
    out.clear();
    assert_eq!(reg[10], 0b10); // CLR pin 1 = 1 << 1

    let pin = gpio.get_pin(35);
    let mut out = pin.into_output();
    out.set();
    assert_eq!(reg[8], 0b1000); // SET pin 35 = 1 << (35 - 32)
    out.clear();
    assert_eq!(reg[11], 0b1000); // CLR pin 35 = 1 << (35 - 32)
}

#[test_case]
#[expect(unused_assignments)]
fn test_pin_inputs() {
    let mut reg = [0u32; 40];
    let mmio_base_addr = Address::<Virtual>::new(&mut reg as *mut _ as usize);
    let gpio = unsafe { GPIO::new(mmio_base_addr) };

    // Modify pin 1
    let pin = gpio.get_pin(1);
    let inp = pin.into_input();

    assert_eq!(inp.level(), false);
    reg[13] = 0b10; // Modify "MMIO" memory
    assert_eq!(inp.level(), true);

    // Modify pin 35
    let pin = gpio.get_pin(35);
    let inp = pin.into_input();

    assert_eq!(inp.level(), false);
    reg[14] = 0b1000; // Modify "MMIO" memory
    assert_eq!(inp.level(), true);
}

// Validate the buffer is filled correctly
// Validate the buffer is properly terminated when call()ed -- this invariant must be maintained
// by the end() fn.
#[test_case]
fn test_prepare_mailbox() {
    use libplatform::platform::device_driver::mailbox::MailboxStorageRef;

    let mut mailbox = Mailbox::<8>::default();
    let index = mailbox.request();
    let index = mailbox.set_led_on(index, true);
    let mailbox = mailbox.end(index);
    // Instead of calling just check the filled buffer format:
    assert_eq!(mailbox.0.buffer.as_ref()[0] as usize, (index + 1) * 4);
    assert_eq!(mailbox.0.buffer.as_ref()[1], mailbox::REQUEST);
    assert_eq!(mailbox.0.buffer.as_ref()[2], tag::SetGpioState);
    assert_eq!(mailbox.0.buffer.as_ref()[3], 8);
    assert_eq!(mailbox.0.buffer.as_ref()[4], 0);
    assert_eq!(mailbox.0.buffer.as_ref()[5], 130);
    assert_eq!(mailbox.0.buffer.as_ref()[6], 1);
    assert_eq!(mailbox.0.buffer.as_ref()[7], tag::End);
}

#[test_case]
fn test_divisors() {
    const CLOCK: u64 = 3_000_000;
    const BAUD_RATE: u32 = 115_200;

    let divisors = RateDivisors::from_clock_and_rate(CLOCK, BAUD_RATE);
    assert!(divisors.is_ok());
    let divisors = divisors.unwrap();
    assert_eq!(divisors.integer_baud_rate_divisor, 1);
    assert_eq!(divisors.fractional_baud_rate_divisor, 40);
}
