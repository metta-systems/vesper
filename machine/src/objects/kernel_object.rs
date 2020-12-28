/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

pub(crate) trait KernelObject {
    fn size_bits() -> usize;
    fn invoke();
}
