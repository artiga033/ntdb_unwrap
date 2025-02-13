use std::path::Path;

pub use sqlite_ext_ntqq_db::*;

use rusqlite::Connection;
use snafu::{whatever, ResultExt};

use crate::{ntqq, SqliteSnafu};

pub fn try_decrypt_db(conn: &Connection, mut d: ntqq::DBDecryptInfo) -> Result<(), crate::Error> {
    let try_alg: [Option<String>; 2] = match d.cipher_hmac_algorithm.take() {
        Some(algo) => [Some(algo), None],
        None => ["HMAC_SHA256", "HMAC_SHA1"].map(|x| Some(x.to_string())),
    };
    for algo in try_alg.into_iter().flatten() {
        println!("trying hmac algorithm: {}", algo);
        d.cipher_hmac_algorithm = Some(algo);
        let stmt = d.display_pragma_stmts().to_string();
        conn.execute_batch(&stmt)
            .context(SqliteSnafu { op: stmt })?;
        let stmt = conn.prepare("SELECT count(*) FROM sqlite_master;");
        match stmt {
            Ok(mut stmt) => {
                if stmt.exists([]).context(SqliteSnafu { op: "stmt.exists" })? {
                    println!("decryped successfully");
                    return Ok(());
                }
            }
            Err(e) => {
                println!("attempt failed: {}", e);
            }
        }
    }
    whatever!("Failed to decrypt database")
}

pub fn export_to_plain(conn: &Connection, file: impl AsRef<Path>) -> crate::Result<()> {
    let stmt = format!(
        r#"ATTACH DATABASE '{}' AS plain KEY ''; 
        SELECT sqlcipher_export('plain');
        DETACH DATABASE plain;"#,
        file.as_ref().display()
    );
    conn.execute_batch(&stmt).context(SqliteSnafu { op: stmt })
}
