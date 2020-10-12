/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */
mod boot;
pub mod memory;

#[inline]
pub fn endless_sleep() -> ! {
    loop {
        cortex_a::asm::wfe();
    }
}
