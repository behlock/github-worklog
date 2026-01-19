pub mod file_ops;
mod generator;

#[allow(deprecated)]
pub use file_ops::append_to_file;
pub use file_ops::{prepend_to_file, WriteResult};
pub use generator::RecapGenerator;
