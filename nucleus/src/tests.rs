/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */
//============================================================================
// Testing environment
//============================================================================
use crate::{println, qemu};

#[cfg(test)]
pub fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests", tests.len());
    for test in tests {
        test();
        println!("[ok]");
    }
    println!("[success]");
    qemu::semihosting::exit_success();
}
