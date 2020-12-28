/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

// implemented for x86 and arm
trait ASIDControl {
    fn make_pool(untyped: Untyped, target_cap_space_cap: CapNodeRootedPath) -> Result<()>;
}
