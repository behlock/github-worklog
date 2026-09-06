pub mod file_ops;
mod generator;

pub use file_ops::{HeaderMatcher, WriteResult, prepend_to_file};
pub use generator::RecapGenerator;
