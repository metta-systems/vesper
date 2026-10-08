# Null

| | |
|---|---|
| Wire type | `0x00` (core) |
| Backing | None |
| Status | Every invocation fails |

## Purpose

Null is the type of an empty capability. Invoking a Null capability fails with `NULL_CAPABILITY` (status 2) before the operation number is read. Null capabilities cannot be created or installed. `KeySlot::NULL` (slot 0) is always empty.

A key to a slot that was never filled, or whose capability was deleted or moved, fails lookup instead; see [key errors](../README.md#key-errors).
