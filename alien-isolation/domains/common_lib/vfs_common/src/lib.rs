#![no_std]
#![forbid(unsafe_code)]
#![feature(allocator_api)]
#![feature(trait_upcasting)]
extern crate alloc;

pub mod id;
pub mod meta;
pub mod shim;
