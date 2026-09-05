#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(format_args_nl)]
#![feature(likely_unlikely)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[path = "../../../tests/common/mod.rs"]
mod common;

// Compile the production module trees, not replacement capability/handler models.
// The binary also allows unused items while these APIs are being brought up.
#[allow(unused)]
#[path = "../src/api/mod.rs"]
mod api;
#[allow(unused)]
#[path = "../src/objects/mod.rs"]
mod objects;

use {
    api::{KeyEntry, debug_console::invoke},
    libobject::{CapError, KeySlot, ObjectType, Rights, domain::DomainId},
    objects::{DebugConsole, KeyTable, Nucleus},
};

#[test_case]
fn rejects_wrong_capability_types_before_touching_write_arguments() {
    for cap in [
        KeyEntry::null(),
        KeyEntry::new_untyped(0, 12, false, Rights::all()),
        KeyEntry::new_frame(0, 12, false, Rights::all()),
    ] {
        for op in [0, u32::MAX] {
            assert!(matches!(
                invoke(&cap, op, u64::MAX, u64::MAX),
                Err(CapError::TypeMismatch { expected, found })
                    if expected == ObjectType::DEBUG_CONSOLE && found == cap.object_type()
            ));
        }
    }
}

#[test_case]
fn rejects_invalid_operations_through_shared_capability_borrows() {
    let console = DebugConsole;
    let cap = KeyEntry::new(&console, Rights::all(), 42);
    let alias = &cap;

    // Invalid opcodes must fail before constructing an address or copying bytes.
    for op in [1, 127, 255, 256, 1 << 16, u32::MAX] {
        assert!(matches!(
            invoke(&cap, op, u64::MAX, u64::MAX),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            invoke(alias, op, u64::MAX, u64::MAX),
            Err(CapError::InvalidOperation)
        ));
    }
    assert_eq!(cap.object_type(), ObjectType::DEBUG_CONSOLE);
    assert_eq!(cap.rights(), Rights::all());
    assert_eq!(cap.badge(), 42);
    assert_eq!(cap.generation(), 0);
}

#[test_case]
fn table_lookup_still_requires_an_installed_capability() {
    let mut table = KeyTable::new(DomainId(0));
    let slot = KeySlot::DEBUG_CONSOLE;
    assert!(matches!(table.lookup(slot), Err(CapError::EmptySlot(s)) if s == slot));
    let invalid = KeySlot(u32::MAX);
    assert!(matches!(table.lookup(invalid), Err(CapError::InvalidSlot(s)) if s == invalid));

    let console = DebugConsole;
    table
        .insert(slot, KeyEntry::new(&console, Rights::all(), 0))
        .unwrap_or_else(|_| panic!("console insertion failed"));
    let cap = table
        .lookup(slot)
        .unwrap_or_else(|_| panic!("installed console not found"));
    assert!(matches!(
        invoke(cap, 1, u64::MAX, u64::MAX),
        Err(CapError::InvalidOperation)
    ));
}
