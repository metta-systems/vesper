#![no_std]

//! Calls to the `pci_v1` interface, generated from
//! `drivers/pcie-brcmstb/interfaces/pci_v1.if`: a `pci_v1::Client` holds the
//! Invocations its component was granted and makes typed calls through them.

include!(concat!(env!("OUT_DIR"), "/pci_v1.rs"));
