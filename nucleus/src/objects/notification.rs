/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

// @todo replace with Event
register_bitfields! {
    u128,
    Notification [
        BoundTCB OFFSET(16) NUMBITS(48) [],
        MsgId OFFSET(64) NUMBITS(64) [],
    ],
    NotificationQueue [
        QueueHead OFFSET(16) NUMBITS(48) [],
        QueueTail OFFSET(64) NUMBITS(48) [],
        State OFFSET(126) NUMBITS(2) [
            Idle = 00b,
            Waiting = 01b,
            Active = 10b,
        ],
    ]
}

trait Notification {
    fn signal(dest: Cap);
    fn wait(src: Cap) -> Result<Option<&Badge>>;
    fn poll(cap: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
}
