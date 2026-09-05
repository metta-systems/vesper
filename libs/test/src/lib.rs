/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */
#![no_std]
#![feature(format_args_nl)]

pub mod panic;

//============================================================================
// Testing environment
//============================================================================

pub trait TestFn {
    fn run(&self);
}

impl<T> TestFn for T
where
    T: Fn(),
{
    fn run(&self) {
        liblog::print!("*TEST* {}...\t", core::any::type_name::<T>());
        self();
        liblog::println!("[ok]\n");
    }
}

pub fn test_runner(tests: &[&dyn TestFn]) {
    liblog::println!("*TESTING* Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    liblog::println!("\n[success]\n");
    libqemu::semihosting::exit_success();
}
