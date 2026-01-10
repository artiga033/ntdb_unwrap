pub mod android;
pub mod windows;

use core::fmt;
use snafu::Snafu;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Android,
    MacOS,
    Unknown,
}
pub fn running_platform() -> Platform {
    #[cfg(target_os = "windows")]
    {
        Platform::Windows
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var("ANDROID_DATA").is_ok() {
            Platform::Android
        } else {
            Platform::Linux
        }
    }
    #[cfg(target_os = "macos")]
    {
        Platform::MacOS
    }
    #[cfg(all(
        not(target_os = "windows"),
        not(target_os = "linux"),
        not(target_os = "macos")
    ))]
    {
        Platform::Unknown
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

pub fn detect_db_file() -> crate::Result<Vec<UserDBFile>> {
    {
        #[cfg(target_os = "windows")]
        {
            windows::detect_db_file()
        }
        #[cfg(target_os = "linux")]
        {
            use crate::UnsupportedPlatformSnafu;

            let platform = running_platform();
            match platform {
                Platform::Linux => UnsupportedPlatformSnafu { platform }.fail(),
                Platform::Android => android::detect_db_file(),
                _ => unreachable!(),
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        UnsupportedPlatformSnafu { platform }.fail()
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

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(transparent)]
    Windows { source: windows::Error },
    #[snafu(transparent)]
    Android { source: android::Error },
}
