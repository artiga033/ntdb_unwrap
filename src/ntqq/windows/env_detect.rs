use crate::ntqq::UserDBFile;
use snafu::{IntoError, OptionExt, ResultExt};
use std::path::PathBuf;

use super::*;

pub(crate) fn detect_db_file() -> crate::Result<Vec<UserDBFile>> {
    let documents = dirs::document_dir().context(LocateDocumentsDirSnafu)?;
    let tencent_files = documents.join("Tencent Files");
    snafu::ensure!(tencent_files.is_dir(), NoTencentFilesDirSnafu);
    // 过滤 tencent files 下的纯数字文件夹名，且其内有"nt_qq\nt_db\nt_msg.db" 子文件的文件夹
    let files = tencent_files
        .read_dir()
        .context(IoSnafu {
            op: "read Tencent Files dir",
        })?
        .filter_map(|d| -> Option<UserDBFile> {
            let d = d.ok()?;
            let Ok(true) = d.file_type().map(|x| x.is_dir()) else {
                return None;
            };
            let uin: u64 = d.file_name().to_string_lossy().parse().ok()?;
            let db_path = d.path().join("nt_qq/nt_db/nt_msg.db");
            if db_path.is_file() {
                Some(UserDBFile {
                    path: db_path,
                    uid: None,
                    uin: Some(uin),
                })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    Ok(files)
}

#[derive(Debug)]
pub struct InstalledQQInfo {
    pub install_dir: PathBuf,
    pub version: Option<String>,
}
pub fn get_installed_qq() -> crate::Result<InstalledQQInfo> {
    // WOW6432Node if any, for 32-bit or 64-bit but upgraded from 32-bit legacy QQ
    let reg = {
        let mut reg = None;
        let read = windows_registry::LOCAL_MACHINE
            .options()
            .read()
            .open(r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\QQ");
        match read {
            Ok(k) => reg = Some(k),
            Err(e) if e.code() != HRESULT(2) => {
                return Err(WindowsOpSnafu {
                    op: "open 32-bit registry key for installed QQ",
                }
                .into_error(e)
                .into());
            }
            Err(_hresult_2) => {}
        }
        if reg.is_none() {
            let read = windows_registry::LOCAL_MACHINE
                .options()
                .read()
                .open(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\NTQQ");
            match read {
                Ok(k) => reg = Some(k),
                Err(e) if e.code() != HRESULT(2) => {
                    return Err(WindowsOpSnafu {
                        op: "open registry key for installed NTQQ",
                    }
                    .into_error(e)
                    .into());
                }
                Err(_hresult_2) => {}
            }
        }
        reg
    };
    let reg = reg.context(QQInstallationNotFoundSnafu)?;
    let uninstall_string = reg.get_string("UninstallString").context(WindowsOpSnafu {
        op: "read UninstallString value from installed QQ registry key",
    })?;
    let uninstall_path = PathBuf::from(uninstall_string);
    let install_dir = uninstall_path
        .parent()
        .context(QQInstallationNotFoundSnafu)?;

    let version = windows_registry::CURRENT_USER
        .options()
        .read()
        .open(r"\HKEY_CURRENT_USER\Software\Tencent\QQNT")
        .and_then(|k| k.get_string("Version"))
        .ok();

    Ok(InstalledQQInfo {
        install_dir: install_dir.to_path_buf(),
        version,
    })
}
