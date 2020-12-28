/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

register_bitfields! {
    u128,
    Endpoint [
        QueueHead OFFSET(0) NUMBITS(64) [],
        QueueTail OFFSET(80) NUMBITS(46) [],
        State OFFSET(126) NUMBITS(2) [
            Idle = 00b,
            Send = 01b,
            Recv = 10b,
        ],
    ],
}
