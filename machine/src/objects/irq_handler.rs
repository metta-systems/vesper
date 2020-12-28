/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

trait IRQHandler {
    fn set_notification(notification: CapNode) -> Result<()>;
    fn ack() -> Result<()>;
    fn clear() -> Result<()>;
}
