pub mod db;
pub mod ntqq;
pub mod util;
mod protos;

use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(context(false))]
    IO { source: std::io::Error },
    #[snafu()]
    Sqlite { source: rusqlite::Error, op: String },
    Protobuf { source: protobuf::Error, raw: Vec<u8> },
    #[snafu(whatever, display("{message}"))]
    Whatever { message: String },
}
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for rusqlite::types::FromSqlError {
    fn from(e: Error) -> Self {
        rusqlite::types::FromSqlError::Other(Box::new(e))
    }
}