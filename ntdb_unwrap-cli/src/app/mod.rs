mod export;
pub use export::*;
mod serve;
pub use serve::*;

mod common;

use crate::Result;
use std::boxed::Box;
pub trait App {
    fn run(self: Box<Self>) -> Result<()>;
}
