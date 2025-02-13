use snafu::whatever;

use crate::util::md5_hex;
use crate::Result;
use core::fmt;
use std::{env, fs, path::PathBuf};

pub enum Platform {
    Windows,
    Linux,
    Android,
}
pub fn get_platform() -> Platform {
    #[cfg(target_os = "windows")]
    {
        Platform::Windows
    }
    #[cfg(target_os = "linux")]
    {
        if env::var("ANDROID_DATA").is_ok() {
            Platform::Android
        } else {
            Platform::Linux
        }
    }
}

#[derive(Debug, Default)]
pub struct UserDBFile {
    pub path: PathBuf,
    pub uid: Option<String>,
    pub uin: Option<u64>,
}
impl fmt::Display for UserDBFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.path.display())?;
        if let Some(uin) = self.uin {
            write!(f, "{}", uin)?;
        } else {
            write!(f, "unknown_uin")?;
        }
        if let Some(uid) = &self.uid {
            write!(f, "({})", uid)?;
        } else {
            write!(f, "(unknown_uid)")?;
        }
        Ok(())
    }
}

pub fn detect_db_file() -> Result<Vec<UserDBFile>> {
    match get_platform() {
        Platform::Windows => {
            whatever!("Auto-detecting db file is not supported on Windows, please specify the db file via command line argument")
        }
        Platform::Linux => {
            whatever!("Auto-detecting db file is not supported on Linux, please specify the db file via command line argument")
        }
        Platform::Android => {
            let data_dir = env::var("ANDROID_DATA").unwrap_or("/data".into());
            let uid_dir = format!("{}/user/0/com.tencent.mobileqq/files/uid", data_dir);
            let uids = fs::read_dir(uid_dir)?;
            let mut files = Vec::with_capacity(uids.size_hint().0);
            for entry in uids {
                let entry = entry?;
                let file_name = entry.file_name();
                if let Some((uin, uid)) = file_name.to_string_lossy().split_once("###") {
                    let uin = uin.parse().unwrap_or_default();
                    let qq_uid_hash = md5_hex(uid);
                    let qq_path_hash = md5_hex(qq_uid_hash + "nt_kernel");
                    files.push(UserDBFile {
                        path: format!(
                            "/data/user/0/com.tencent.mobileqq/databases/nt_db/nt_qq_{}/nt_msg.db",
                            qq_path_hash
                        )
                        .into(),
                        uid: Some(uid.to_string()),
                        uin: Some(uin),
                    });
                }
            }
            Ok(files)
        }
    }
}
#[derive(Debug, Default)]
pub struct DBDecryptInfo {
    /// Acutually should be be represented as a Vec<u8>.
    /// However it seems it's always printable strings on most platform known.
    /// So it's even more convenient to just use a String.
    pub key: String,
    /// one of HMAC_SHA1 or HMAC_SHA256
    /// if None, should try all possible algorithms.  
    /// And you must set this field to a `Some` value before displaying,
    /// or [DBDecryptInfo::display_pragma_stmts] will display an error.
    pub cipher_hmac_algorithm: Option<String>,
}

pub struct DisplayPragmaStmts<'a>(&'a DBDecryptInfo);

impl fmt::Display for DisplayPragmaStmts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "PRAGMA key = '{}';", self.0.key)?;
        writeln!(
            f,
            "PRAGMA cipher_page_size = {};",
            DBDecryptInfo::CIPHER_PAGE_SIZE
        )?;
        writeln!(f, "PRAGMA kdf_iter = {};", DBDecryptInfo::KDF_ITER)?;
        writeln!(
            f,
            "PRAGMA cipher_hmac_algorithm = {};",
            self.0.cipher_hmac_algorithm.as_ref().ok_or(fmt::Error)?
        )?;
        writeln!(
            f,
            "PRAGMA cipher_default_kdf_algorithm = {};",
            DBDecryptInfo::CIPHER_DEFAULT_KDF_ALGORITHM
        )?;
        writeln!(f, "PRAGMA cipher = '{}';", DBDecryptInfo::CIPHER)?;
        Ok(())
    }
}
impl DBDecryptInfo {
    const CIPHER_PAGE_SIZE: usize = 4096;
    const KDF_ITER: usize = 4000;
    const CIPHER_DEFAULT_KDF_ALGORITHM: &str = "PBKDF2_HMAC_SHA512";
    const CIPHER: &str = "aes-256-cbc";

    pub fn display_pragma_stmts(&self) -> DisplayPragmaStmts<'_> {
        DisplayPragmaStmts(self)
    }
}

pub mod android {
    use crate::util::md5_hex;

    /// Decode the header of the db file.
    /// Normally you should pass the first 1024 bytes of the db file.
    ///
    /// Result is the `rand` value as explained [here](https://github.com/QQBackup/qq-win-db-key/blob/master/%E6%95%99%E7%A8%8B%20-%20NTQQ%20(Android).md#%E8%8E%B7%E5%8F%96%E5%AF%86%E9%92%A5:~:text=%E8%B7%9F%E9%9A%8F%E5%9C%A8QQ_NT%20DB%E5%90%8E%E7%9A%84%E5%8F%AF%E8%AF%BB%E5%AD%97%E7%AC%A6%E4%B8%B2%E5%A4%8D%E5%88%B6%EF%BC%8C%E5%BD%A2%E5%A6%826tPaJ9GP%EF%BC%8C%E8%AE%B0%E4%B8%BArand)
    pub fn decode_db_header(uid: &str, bytes: &[u8]) -> Option<super::DBDecryptInfo> {
        let qq_uid_hash = md5_hex(uid);
        enum State {
            /// scan for 'QQNT' pattern
            MatchingQqntPat,
            /// scan for 'DB' pattern
            MatchingDbPat,
            /// scan for 8-byte or more ascii-printable bytes
            ReadingRand,
        }
        let mut state = State::MatchingQqntPat;
        let mut wnd = Vec::with_capacity(8);
        macro_rules! switch_state {
            ($new_state:expr) => {
                state = $new_state;
                wnd.clear();
            };
        }
        for byte in bytes {
            match state {
                State::MatchingQqntPat => {
                    if wnd.len() == 4 {
                        switch_state!(State::MatchingDbPat);
                    } else if b"QQNT"[wnd.len()] == *byte {
                        wnd.push(*byte);
                    }
                }
                State::MatchingDbPat => {
                    if wnd.len() == 2 {
                        switch_state!(State::ReadingRand);
                    } else if b"DB"[wnd.len()] == *byte {
                        wnd.push(*byte);
                    } else {
                        // clear the window, because here we want a continuous sequence of 'DB'
                        wnd.clear();
                    }
                }
                State::ReadingRand => {
                    if byte.is_ascii_graphic() {
                        wnd.push(*byte);
                    } else if wnd.len() >= 8 {
                        let rand = String::from_utf8(wnd).unwrap();
                        let key = md5_hex(qq_uid_hash + &rand);
                        return Some(super::DBDecryptInfo {
                            key,
                            cipher_hmac_algorithm: None,
                        });
                    } else {
                        wnd.clear();
                    }
                }
            }
        }
        None
    }
}
