pub mod adapters;
pub mod compiler;
pub mod eip712;
pub mod error;
pub mod ir;
pub mod output;
pub mod registry;
pub mod schema;

pub use compiler::compile;
pub use output::{CompileOutput, CompileOutputJson, CompileResult};
