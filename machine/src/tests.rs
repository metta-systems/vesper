// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
// SPDX-FileCopyrightText: 2024 Metta Systems OÜ
// SPDX-FileContributor: Berkus
//
// SPDX-License-Identifier: BlueOak-1.0.0

//============================================================================
// Testing environment
//============================================================================
use crate::{print, println, qemu};

pub trait TestFn {
    fn run(&self);
}

impl<T> TestFn for T
where
    T: Fn(),
{
    fn run(&self) {
        print!("*TEST* {}...\t", core::any::type_name::<T>());
        self();
        println!("[ok]\n");
    }
}

pub fn test_runner(tests: &[&dyn TestFn]) {
    println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    println!("\n[success]\n");
    qemu::semihosting::exit_success();
}
