pub mod db;
pub mod ntqq;
pub mod util;

use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(context(false))]
    IO { source: std::io::Error },
    #[snafu()]
    Sqlite { source: rusqlite::Error, op: String },
    #[snafu(whatever, display("{message}"))]
    Whatever { message: String },
}
pub type Result<T> = std::result::Result<T, Error>;
