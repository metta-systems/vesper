use core::hint::spin_loop;

/// Loop for a given number of `nop` instructions.
#[inline]
pub fn loop_delay(rounds: u32) {
    for _ in 0..rounds {
        spin_loop();
    }
}

/// Loop until a passed function returns `true`.
#[inline]
pub fn loop_until<F: Fn() -> bool>(f: F) {
    loop {
        if f() {
            break;
        }
        spin_loop();
    }
}

/// Loop while a passed function returns `true`.
#[inline]
pub fn loop_while<F: Fn() -> bool>(f: F) {
    loop {
        if !f() {
            break;
        }
        spin_loop();
    }
}
