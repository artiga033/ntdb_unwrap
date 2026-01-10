use super::*;
use crate::util::md5_hex;
use std::{env, fs};

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

pub fn detect_db_file() -> crate::Result<Vec<UserDBFile>> {
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
