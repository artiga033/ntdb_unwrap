pub mod db;
pub mod ntqq;
mod protos;
pub mod util;

use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    Protobuf {
        source: protobuf::Error,
        raw: Vec<u8>,
    },
    #[snafu(transparent)]
    DB{
        source: db::Error,
    },
    #[snafu(transparent)]
    NTQQ {
        source: ntqq::Error,
    },
    UnsupportedPlatform {
        platform: ntqq::Platform,
    },
}
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for rusqlite::types::FromSqlError {
    fn from(e: Error) -> Self {
        rusqlite::types::FromSqlError::Other(Box::new(e))
    }
}
