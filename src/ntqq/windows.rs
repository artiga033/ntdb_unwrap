#[cfg(target_os = "windows")]
mod debug_process;
#[cfg(target_os = "windows")]
pub use debug_process::{DebugInfo, debug_for_key};
#[cfg(target_os = "windows")]
mod env_detect;
#[cfg(target_os = "windows")]
pub(crate) use env_detect::detect_db_file;
#[cfg(target_os = "windows")]
pub use env_detect::{InstalledQQInfo, get_installed_qq};
mod static_analysis;
pub use static_analysis::TargetFunction;

use snafu::Snafu;

#[cfg(target_os = "windows")]
use windows::core::HRESULT;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("no Documents dir located"))]
    LocateDocumentsDir,
    #[snafu(display("no Tencent Files dir located in Documents"))]
    NoTencentFilesDir,
    #[snafu(display("installed QQ not found"))]
    QQInstallationNotFound,
    #[snafu(display("cannot find the installed QQ version"))]
    LocateInstalledQQVersion,
    #[snafu(display("IO error when {}: {}", op, source))]
    Io {
        source: std::io::Error,
        op: String,
    },
    #[cfg(target_os = "windows")]
    #[snafu(transparent)]
    Windows {
        source: ::windows::core::Error,
    },
    #[cfg(target_os = "windows")]
    #[snafu(display("Windows error when {}: {}", op, source))]
    WindowsOp {
        source: ::windows::core::Error,
        op: String,
    },
    #[snafu(transparent)]
    Object {
        source: object::Error,
    },
    #[snafu(transparent)]
    Capstone {
        source: capstone::Error,
    },
    #[snafu(display("find target function offset: {}", msg))]
    FindTargetFunction {
        msg: String,
    },
    #[snafu(display("debug for key: {}", msg))]
    DebugForKey {
        msg: String,
    },
    ReadRemoteString {
        msg: String,
    },
    #[cfg(target_os = "windows")]
    UnsupportedArchitecture {
        arch: windows::System::ProcessorArchitecture,
    },
}

impl From<Error> for crate::Error {
    fn from(value: Error) -> Self {
        super::Error::from(value).into()
    }
}

#[allow(dead_code)]
type Result<T> = std::result::Result<T, Error>;
