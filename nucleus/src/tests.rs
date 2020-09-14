//============================================================================
// Testing environment
//============================================================================
use crate::println;

#[cfg(test)]
pub fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
    println!("[success]");
    qemu_exit::aarch64::exit_success();
}
