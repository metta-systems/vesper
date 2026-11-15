//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The rings (§4.9): where on a ring of TRBs the next one goes, and which
//! cycle bit it carries. A ring here is one segment of `len` TRBs at a bus
//! address; the memory itself is the driver's.

use crate::trb::{TRB_BYTES, Trb};

/// Where to write one TRB: its index in the segment, the TRB with its cycle
/// bit, and its bus address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub index: usize,
    pub trb: Trb,
    pub bus: u64,
}

/// A ring software produces into, the command ring or a transfer ring
/// (§4.9.2): `len` TRBs, the last of them a Link TRB back to the first that
/// toggles the cycle state. The producer cycle state starts at 1, which is
/// what `CRCR.RCS` is set to.
#[derive(Clone, Copy, Debug)]
pub struct Producer {
    base: u64,
    len: usize,
    enqueue: usize,
    cycle: bool,
}

impl Producer {
    /// A ring of `len` TRBs (at least two: one TRB and the link) at bus
    /// address `base`, all zero, so the controller owns none of them.
    #[must_use]
    pub const fn new(base: u64, len: usize) -> Self {
        assert!(len >= 2, "a ring needs room for a TRB and its link");
        Self {
            base,
            len,
            enqueue: 0,
            cycle: true,
        }
    }

    /// The ring's bus address and its cycle state at the start, for `CRCR`.
    #[must_use]
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// Place `trb` at the enqueue pointer with the producer cycle bit. When
    /// that fills the segment up to its last TRB, the Link TRB to write after
    /// it comes second: it carries the cycle bit of the pass it ends, and the
    /// next TRB goes at the start with the cycle bit flipped. Write the TRB
    /// before its link.
    pub fn push(&mut self, trb: Trb) -> (Placement, Option<Placement>) {
        let placed = self.placement(self.enqueue, trb);
        self.enqueue += 1;
        let link = (self.enqueue == self.len - 1).then(|| {
            let link = self.placement(self.len - 1, Trb::link(self.base));
            self.enqueue = 0;
            self.cycle = !self.cycle;
            link
        });
        (placed, link)
    }

    fn placement(&self, index: usize, trb: Trb) -> Placement {
        Placement {
            index,
            trb: trb.with_cycle(self.cycle),
            bus: self.base + index as u64 * TRB_BYTES,
        }
    }
}

/// The event ring, which the controller produces into and software consumes
/// (§4.9.4): one segment of `len` TRBs. The consumer cycle state starts at 1:
/// a TRB is the controller's new event while its cycle bit equals it.
#[derive(Clone, Copy, Debug)]
pub struct Consumer {
    base: u64,
    len: usize,
    dequeue: usize,
    cycle: bool,
}

impl Consumer {
    /// The event ring segment of `len` TRBs at bus address `base`, all zero.
    #[must_use]
    pub const fn new(base: u64, len: usize) -> Self {
        assert!(len >= 1, "an event ring segment needs a TRB");
        Self {
            base,
            len,
            dequeue: 0,
            cycle: true,
        }
    }

    /// The index of the next TRB to read.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.dequeue
    }

    /// Whether `trb`, read at [`Self::index`], is an event the controller
    /// wrote and software has not consumed.
    #[must_use]
    pub const fn owns(&self, trb: &Trb) -> bool {
        trb.cycle() == self.cycle
    }

    /// Step past the TRB at [`Self::index`]; past the segment's end the
    /// dequeue pointer wraps and the cycle state flips.
    pub const fn advance(&mut self) {
        self.dequeue += 1;
        if self.dequeue == self.len {
            self.dequeue = 0;
            self.cycle = !self.cycle;
        }
    }

    /// The bus address `ERDP` takes: the next TRB to read.
    #[must_use]
    pub const fn dequeue_pointer(&self) -> u64 {
        self.base + self.dequeue as u64 * TRB_BYTES
    }
}
