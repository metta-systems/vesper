//============================================================================
// Testing environment
//============================================================================
use crate::{println, qemu};

#[cfg(test)]
pub fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
    println!("[success]");
    qemu::semihosting::exit_success();
}
