//! Scarlet Native platform abstraction layer.
//!
//! This module is intentionally a buildable skeleton for the initial Scarlet
//! target bring-up. M2 replaces the unsupported stubs with Scarlet Native ABI
//! syscalls through the extracted `scarlet-sys` and `scarlet-abi` layers.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod abi;
pub mod os;
pub mod time;

mod common;
pub use common::*;
