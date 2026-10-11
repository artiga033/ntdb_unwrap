use crate::{Error, Result, *};
use clap::ArgMatches;
use ntdb_unwrap::ntqq::{Platform, running_platform};
use ntdb_unwrap::*;
use ntqq::{DBDecryptInfo, UserDBFile};
use rusqlite::{Connection, OpenFlags};
use snafu::{FromString, prelude::*};
#[cfg(feature = "serve")]
use std::mem::ManuallyDrop;
use std::{env, fs, io::Read};

/// Select the source, obtain credentials and prepare the original temporary-file copy without opening SQLite.
///
/// 1. parse the `matches` arguments which MUST include all the defined [crate::common_args] arguments.
/// 2. use the `file` argument to detect the as database file, if none, try auto detect, and interactively asks user to choose one.
/// 3. use the `pkey` argument to decrypt the database file, if none, try auto detect.
/// 4. if `nocopy` flag is not set, copy the database file to a temp file, and use the temp file as the database file.
pub fn prepare(matches: &ArgMatches) -> Result<Prepared> {
    let offset = matches.get_one::<i64>("pending-byte-offset").copied();
    if let Some(v) = offset {
        db::pending::value(v).map_err(Error::without_source)?;
    }
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
            if db_files.is_empty() {
                whatever!("无法自动检测到数据库文件，请通过命令行参数手动指定");
            } else if db_files.len() == 1 {
                db_files.into_iter().next().unwrap()
            } else {
                println!("选择要使用的数据库文件：");
                for (i, db_file) in db_files.iter().enumerate() {
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
        None => get_decrypt_info(&file, running_platform())?,
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
    let prepared = Prepared {
        user_db_file: file,
        decrypt_info,
        offset,
        working_on_temp_file,
    };
    #[cfg(windows)]
    if db::job::cancelled() {
        return Err(Error::without_source("任务已取消".into()));
    }
    Ok(prepared)
}

pub struct Prepared {
    pub user_db_file: UserDBFile,
    pub decrypt_info: DBDecryptInfo,
    pub offset: Option<i64>,
    pub working_on_temp_file: bool,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if self.working_on_temp_file {
            println!("清理临时文件: {:?}", self.user_db_file.path);
            fs::remove_file(&self.user_db_file.path).unwrap();
        }
    }
}
#[cfg(feature = "serve")]
pub struct Bootstrap {
    pub conn: ManuallyDrop<Connection>,
    pub prepared: Prepared,
}
#[cfg(feature = "serve")]
impl std::fmt::Debug for Bootstrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bootstrap { read_only: true }")
    }
}
#[cfg(feature = "serve")]
impl Drop for Bootstrap {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.conn);
        }
    }
}
#[cfg(feature = "serve")]
pub fn bootstrap(matches: &ArgMatches) -> Result<Bootstrap> {
    let prepared = prepare(matches)?;
    unsafe {
        db::pending::configure(prepared.offset).map_err(Error::without_source)?;
    }
    let conn = match open_source(&prepared.user_db_file.path, &prepared.decrypt_info) {
        Ok(conn) => conn,
        Err(e) => {
            let config = super::reader::Config::from_prepared(&prepared);
            let message = e.to_string();
            if let Some(hint) = super::reader::diagnose(&config, &message) {
                eprintln!("{hint}");
            }
            return Err(e);
        }
    };
    Ok(Bootstrap {
        conn: ManuallyDrop::new(conn),
        prepared,
    })
}
pub fn open_source(path: &std::path::Path, info: &DBDecryptInfo) -> Result<Connection> {
    db::register_offset_vfs()
        .map_err(|e| Error::without_source(format!("offset VFS registration failed: {e}")))?;
    let conn = Connection::open_with_flags_and_vfs(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
        db::OFFSET_VFS_NAME,
    )
    .context(SqliteSnafu {
        op: "open read-only source",
    })?;
    let info = DBDecryptInfo {
        key: info.key.clone(),
        cipher_hmac_algorithm: info.cipher_hmac_algorithm.clone(),
    };
    db::try_decrypt_db(&conn, info)?;
    Ok(conn)
}
/// 此函数对不同平台的行为不同。
/// - Android: 仅仅是简单的计算，可以很快完成。
/// - Windows: 需要启动一个 QQ 进程并附加调试器，这需要用户操作，且会长时间阻塞。
///
/// **注意**: 对于任何异步调用者，应当将此函数视为长时阻塞调用。
fn get_decrypt_info(file: &UserDBFile, platform: Platform) -> Result<DBDecryptInfo> {
    match platform {
        Platform::Android => {
            if let Some(uid) = &file.uid {
                let mut f = fs::File::open(&file.path)?;
                let mut buf = [0u8; 1024];
                f.read_exact(&mut buf)?;
                ntqq::android::decode_db_header(uid, &buf)
                    .whatever_context::<_, Error>("decode android nt_qq db header")
            } else {
                whatever!("Android平台下必须提供UID以自动解密数据库");
            }
        }
        Platform::Windows => {
            #[cfg(target_os = "windows")]
            {
                use ntdb_unwrap::ntqq::windows::{DebugInfo, debug_for_key};
                let qq = ntqq::windows::get_installed_qq()?;
                println!("检测到已安装的QQ: {:?}", qq);
                let func = ntqq::windows::TargetFunction::from_installed_qq(&qq)?;
                println!(
                    "引用特征字符串的 LEA 指令地址: 0x{:X}",
                    func.lea_instr_offset
                );
                println!("指令所在函数开始地址：0x{:X}", func.function_offset);
                println!("启动QQ进程并附加调试器以提取解密密钥...");
                println!("请在新打开的QQ窗口正登录目标账号后，等待程序自动完成解密密钥提取。");
                let decrypt_info = debug_for_key(&DebugInfo { qq, func })?;
                println!("解密密钥提取完成: {}", decrypt_info.key);
                Ok(decrypt_info)
            }
            #[cfg(not(target_os = "windows"))]
            {
                whatever!(
                    "Windows的密钥自动提取仅在Windows上运行时可用，当前平台: {:?}",
                    platform
                )
            }
        }
        _ => {
            whatever!("此平台({:?})不支持自动解密，请手动提供解密密钥", platform);
        }
    }
}
