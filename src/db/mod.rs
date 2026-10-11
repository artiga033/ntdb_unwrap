#[cfg(windows)]
pub mod job;
pub mod model;
pub mod pending;
pub mod stream;

use std::path::Path;

pub use sqlite_ext_ntqq_db::*;

use rusqlite::Connection;
use snafu::{ResultExt, Snafu};

use crate::ntqq;

pub fn try_decrypt_db(conn: &Connection, mut d: ntqq::DBDecryptInfo) -> crate::Result<()> {
    let mut corrupt = None;
    let try_alg: [Option<String>; 2] = match d.cipher_hmac_algorithm.take() {
        Some(algo) => [Some(algo), None],
        None => ["HMAC_SHA256", "HMAC_SHA1"].map(|x| Some(x.to_string())),
    };
    for algo in try_alg.into_iter().flatten() {
        log::info!("trying hmac algorithm: {}", algo);
        d.cipher_hmac_algorithm = Some(algo);
        let stmt = d.display_pragma_stmts().to_string();
        conn.execute_batch(&stmt)
            .context(SqliteSnafu { op: stmt })?;
        let stmt = conn.prepare("SELECT count(*) FROM sqlite_master;");
        match stmt {
            Ok(mut stmt) => {
                if stmt.exists([]).context(SqliteSnafu { op: "stmt.exists" })? {
                    log::info!("decryped successfully");
                    return Ok(());
                }
            }
            Err(e) => {
                log::info!("attempt failed: {}", e);
                if matches!(&e,rusqlite::Error::SqliteFailure(code,_) if code.code==rusqlite::ErrorCode::DatabaseCorrupt)
                {
                    corrupt = Some(e);
                }
            }
        }
    }
    if let Some(source) = corrupt {
        return Err(Error::Sqlite {
            source,
            op: "read source schema".into(),
        }
        .into());
    }
    WrongDecryptInfoSnafu.fail().map_err(crate::Error::from)
}

/// Synchronous record export with independent source and destination connections.
/// Nonstandard pending-byte layouts require process isolation (the CLI provides it).
pub fn export_to_plain(conn: &Connection, file: impl AsRef<Path>) -> crate::Result<()> {
    stream::export_connection(conn, file.as_ref()).map_err(|e| {
        Error::Export {
            message: e.to_string(),
        }
        .into()
    })
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{op}: {source}"))]
    Sqlite {
        source: rusqlite::Error,
        op: String,
    },
    WrongDecryptInfo {},
    #[snafu(display("{message}"))]
    Export {
        message: String,
    },
}
