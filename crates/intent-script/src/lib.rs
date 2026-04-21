#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub mod adapters;
pub mod compiler;
pub mod eip712;
pub mod error;
pub mod ir;
pub mod output;
pub mod registry;
pub mod schema;

pub use compiler::{compile, compile_with_allowances};
pub use output::{CompileOutput, CompileOutputJson, CompileResult};
