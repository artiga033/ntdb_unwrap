use ntdb_unwrap::{
    db::{self, OFFSET_VFS_NAME, model::Model, register_offset_vfs, try_decrypt_db},
    ntqq::DBDecryptInfo,
};
use rusqlite::{Connection, fallible_streaming_iterator::FallibleStreamingIterator};

fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    if args.len() < 3 {
        eprintln!("Usage: {} <dbfile> [pkey]", args[0]);
        std::process::exit(1);
    }
    let mut iter = args.into_iter();
    iter.next();
    let dbfile = iter.next().unwrap();
    let key = iter.next().unwrap();

    register_offset_vfs().expect("Failed to register offset_vfs");
    let conn = Connection::open(format!("file:{}?vfs={}", dbfile, OFFSET_VFS_NAME))
        .expect("Failed to open db");

    try_decrypt_db(
        &conn,
        DBDecryptInfo {
            key,
            // set to None to automatically guess
            cipher_hmac_algorithm: None,
        },
    )
    .expect("Failed to decrypt db");

    let mut stmt = conn
        .prepare("SELECT * FROM group_msg_table ORDER BY `40050` DESC LIMIT 10;")
        .expect("prepare stmt failed");
    stmt.query([])
        .unwrap()
        .for_each(|row| {
            let m = db::model::GroupMsgTable::parse_row(row).expect("Failed to parse row");
            println!("{}", serde_json::to_string_pretty(&m).unwrap());
        })
        .expect("Failed to query");
}
