//! A second build of `coverage-ledger`, used only by the upgrade tests.
//!
//! It has the same exported functions and storage layout as the real contract,
//! but a different wasm hash, so a test can move a live instance from one
//! executable to the other and then read the state it wrote beforehand.
#![no_std]
#![cfg(not(test))]

#[allow(unused_attributes)]
#[path = "../../coverage-ledger/src/lib.rs"]
mod real;
