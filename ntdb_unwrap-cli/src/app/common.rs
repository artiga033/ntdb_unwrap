use crate::{Error, Result, *};
use clap::ArgMatches;
use ntdb_unwrap::*;
use ntqq::{DBDecryptInfo, UserDBFile};
use rusqlite::{Connection, OpenFlags};
use snafu::{FromString, prelude::*};
use std::mem::ManuallyDrop;
use std::{env, fs, io::Read};

/// Common App bootstrap logic, includes:
///
/// 1. parse the `matches` arguments which MUST include all the defined [crate::common_args] arguments.
/// 2. use the `file` argument to detect the as database file, if none, try auto detect, and interactively asks user to choose one.
/// 3. use the `pkey` argument to decrypt the database file, if none, try auto detect.
/// 4. if `nocopy` flag is not set, copy the database file to a temp file, and use the temp file as the database file.
/// 5. open the database file with the offset vfs, and try decrypt it.
pub fn bootstrap(matches: &ArgMatches) -> Result<Bootstrap> {
    let mut file: UserDBFile = match matches.get_one::<String>("file") {
        Some(f) => UserDBFile {
            path: fs::canonicalize(f)?,
            uid: matches
                .get_one::<String>("android-uid")
                .map(ToOwned::to_owned),
            ..Default::default()
        },
        None => {
            let db_files = ntqq::detect_db_file()?;
            if db_files.len() == 1 {
                db_files.into_iter().next().unwrap()
            } else {
                for (i, db_file) in db_files.iter().enumerate() {
                    println!("选择要使用的数据库文件：");
                    println!("{}. {}", i, db_file);
                }
                let mut input = String::new();
                loop {
                    input.clear();
                    std::io::stdin().read_line(&mut input).unwrap();
                    if let Ok(i) = input.trim().parse::<usize>()
                        && i < db_files.len()
                    {
                        break db_files.into_iter().nth(i).unwrap();
                    }
                    println!("无效输入，请重试：");
                }
            }
        }
    };

    let decrypt_info: DBDecryptInfo = match matches.get_one::<String>("pkey") {
        Some(pkey) => DBDecryptInfo {
            key: pkey.to_owned(),
            ..Default::default()
        },
        None => {
            if let Some(uid) = &file.uid {
                let mut f = fs::File::open(&file.path)?;
                let mut buf = [0u8; 1024];
                f.read_exact(&mut buf)?;
                ntqq::android::decode_db_header(uid, &buf)
                    .whatever_context::<_, Error>("failed to decode android nt_qq db header")?
            } else {
                whatever!("自动解密失败，请手动提供密钥");
            }
        }
    };

    let mut working_on_temp_file = false;
    if !matches.get_flag("nocopy") {
        let temp_file = env::temp_dir().join("nt_msg_temp_copy.db");
        println!("复制数据库文件为临时文件：{:?}", temp_file);
        fs::copy(&file.path, &temp_file)?;
        working_on_temp_file = true;
        file.path = temp_file;
    } else {
        println!("[WARN] 正在直接操作原始数据库文件，这可能会损坏你的数据!");
    }

    db::register_offset_vfs().map_err(|e| {
        Error::without_source(format!("failed to register offset vfs: sqlite code {}", e))
    })?;
    let conn = rusqlite::Connection::open_with_flags_and_vfs(
        &file.path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        db::OFFSET_VFS_NAME,
    )
    .context(SqliteSnafu { op: "open db" })?;

    db::try_decrypt_db(&conn, decrypt_info)?;
    Ok(Bootstrap {
        user_db_file: file,
        conn: ManuallyDrop::new(conn),
        working_on_temp_file,
    })
}
/// Common App bootstrap struct.
///
/// If working on temp file, please note that the temp file is deleted as long as this struct is dropped.  
/// [Drop::drop] is triggered when the struct is out of scope. That means you should never partial move or deconstruct this struct. Instead, always **use it as a whole**.
///
#[derive(Debug)]
pub struct Bootstrap {
    pub user_db_file: UserDBFile,
    pub conn: ManuallyDrop<Connection>,
    pub working_on_temp_file: bool,
}
impl Drop for Bootstrap {
    /// by default, [drop] will drop the struct first, and then the fields.
    /// However, here we need to drop the connection first, and then the temp file.
    /// because if we first delete the file, then we cannot successfully close the connection.
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.conn);
        }
        if self.working_on_temp_file {
            println!("清理临时文件: {:?}", self.user_db_file.path);
            fs::remove_file(&self.user_db_file.path).unwrap();
        }
    }
}
