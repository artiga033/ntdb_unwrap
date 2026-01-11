use crate::ntqq::DBDecryptInfo;

use super::*;

pub fn debug_for_key(_info: &DebugInfo) -> crate::Result<DBDecryptInfo> {
    Err(UnsupportedArchitectureSnafu {
        arch: windows::System::ProcessorArchitecture::Arm64,
    }
    .build()
    .into())
}
