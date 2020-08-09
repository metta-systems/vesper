/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */
mod boot;

#[inline]
pub fn endless_sleep() -> ! {
    loop {
        cortex_a::asm::wfe();
    }
}
