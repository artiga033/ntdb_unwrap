mod export;
pub use export::*;
#[cfg(feature = "serve")]
mod serve;
#[cfg(feature = "serve")]
pub use serve::*;

mod common;
pub mod reader;

use crate::Result;
use std::boxed::Box;
pub trait App {
    fn run(self: Box<Self>) -> Result<()>;
}
