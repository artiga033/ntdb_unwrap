/// Input information for the debug-based key extraction.
pub struct DebugInfo {
    /// Information about the installed QQ instance.
    pub qq: InstalledQQInfo,
    /// Target function to set breakpoint on.
    pub func: TargetFunction,
}

#[cfg(target_arch = "x86_64")]
include!("debug_process_x86_64.rs");
#[cfg(target_arch = "aarch64")]
include!("debug_process_aarch64.rs");
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
include!("debug_process_unsupported.rs");
