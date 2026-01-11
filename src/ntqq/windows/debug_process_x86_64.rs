// TODO: most code and logic of this module are done in help or totally by LLMs, needs further review.

use crate::ntqq::DBDecryptInfo;
use log::{debug, error, info};
use snafu::prelude::*;
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::mem::size_of;
use windows::Win32::{
    Foundation::*,
    System::{
        Diagnostics::{Debug::*, ToolHelp::*},
        Threading::*,
    },
};
use windows::core::PSTR;

use super::*;

/// Software breakpoint management structure.
struct SoftwareBreakpoint {
    address: u64,
    original_byte: u8,
    is_active: bool,
}

/// Thread single-step state management.
struct ThreadStepState {
    breakpoint_address: u64,
}

/// RAII wrapper for Windows HANDLE that closes on drop.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: We own this handle and it's valid.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

impl OwnedHandle {
    /// Creates a new owned handle. Returns `None` if the handle is invalid.
    fn new(handle: HANDLE) -> Option<Self> {
        if handle.is_invalid() {
            None
        } else {
            Some(Self(handle))
        }
    }

    /// Returns the raw handle.
    const fn as_raw(&self) -> HANDLE {
        self.0
    }
}

/// Launches a QQ process, attaches a debugger, sets breakpoints, and extracts the key.
///
/// # Errors
///
/// Returns an error if:
/// - QQ executable is not found
/// - Process creation fails
/// - Breakpoint setting fails
/// - Key extraction fails
///
/// # Panics
///
/// Panics if the process handle is invalid after a successful `CreateProcessA` call,
/// which should never happen in practice.
#[allow(clippy::too_many_lines)]
pub fn debug_for_key(info: &DebugInfo) -> crate::Result<DBDecryptInfo> {
    let qq_exe = info.qq.install_dir.join("QQ.exe");
    snafu::ensure!(qq_exe.is_file(), QQInstallationNotFoundSnafu);

    let process_info = create_debug_process(&qq_exe)?;
    let process_handle = OwnedHandle::new(process_info.hProcess)
        .expect("process handle should be valid after successful CreateProcessA");

    let mut debug_event = DEBUG_EVENT::default();
    let mut wrapper_base = 0u64;
    let target_rva = info.func.function_offset;
    let mut breakpoint: Option<SoftwareBreakpoint> = None;
    let mut stepping_threads: HashMap<u32, ThreadStepState> = HashMap::new();

    info!(
        "Starting debug loop for QQ process PID: {}",
        process_info.dwProcessId
    );

    loop {
        // Wait for debug event with 10 second timeout
        let wait_result = unsafe { WaitForDebugEvent(&mut debug_event, 10_000) };

        if wait_result.is_err() {
            error!("WaitForDebugEvent failed: {:?}", wait_result);
            continue;
        }

        let mut continue_status = DBG_CONTINUE;
        let mut should_continue = true;

        match debug_event.dwDebugEventCode {
            EXCEPTION_DEBUG_EVENT => {
                let result = handle_exception_event(
                    &debug_event,
                    &mut breakpoint,
                    &mut stepping_threads,
                    process_handle.as_raw(),
                )?;

                match result {
                    ExceptionResult::KeyFound(key) => {
                        terminate_process(process_handle.as_raw())?;
                        info!("Target terminated. Job done.");
                        return Ok(DBDecryptInfo {
                            key,
                            cipher_hmac_algorithm: None,
                        });
                    }
                    ExceptionResult::Continue(status) => {
                        continue_status = status;
                    }
                }
            }

            LOAD_DLL_DEBUG_EVENT => {
                if wrapper_base == 0
                    && let Ok(base) = get_module_base(debug_event.dwProcessId, "wrapper.node")
                {
                    wrapper_base = base;
                    let target_addr = wrapper_base + target_rva;
                    info!("wrapper.node found at: 0x{wrapper_base:X}, target: 0x{target_addr:X}");

                    match set_software_breakpoint(process_handle.as_raw(), target_addr) {
                        Ok(original_byte) => {
                            breakpoint = Some(SoftwareBreakpoint {
                                address: target_addr,
                                original_byte,
                                is_active: true,
                            });
                            info!("Software breakpoint set at 0x{target_addr:X}");
                        }
                        Err(e) => {
                            error!("Failed to set software breakpoint: {e}");
                        }
                    }
                }
            }

            CREATE_THREAD_DEBUG_EVENT => {
                debug!("Thread created: {}", debug_event.dwThreadId);
            }

            CREATE_PROCESS_DEBUG_EVENT => {
                info!("Process created. PID: {}", debug_event.dwProcessId);
            }

            EXIT_PROCESS_DEBUG_EVENT => {
                let exit_code = unsafe { debug_event.u.ExitProcess.dwExitCode };
                info!("Process exited with code: {}", exit_code);
                should_continue = false;
            }

            _ => {}
        }

        unsafe {
            ContinueDebugEvent(
                debug_event.dwProcessId,
                debug_event.dwThreadId,
                continue_status,
            )
            .context(WindowsOpSnafu {
                op: "continue debug event",
            })?;
        }

        if !should_continue {
            break;
        }
    }

    error!("Debug loop exited without finding key");
    Err(DebugForKeySnafu {
        msg: "failed to find encryption key",
    }
    .build()
    .into())
}

/// Result of handling an exception event.
enum ExceptionResult {
    /// Key was successfully extracted.
    KeyFound(String),
    /// Continue debugging with the specified status.
    Continue(NTSTATUS),
}

/// Creates a QQ process with debugging enabled.
fn create_debug_process(qq_exe: &std::path::Path) -> Result<PROCESS_INFORMATION> {
    let startupinfo = STARTUPINFOA::default();
    let mut process_info = PROCESS_INFORMATION::default();

    let mut path = qq_exe.to_string_lossy().into_owned();
    path.push('\0');

    unsafe {
        CreateProcessA(
            PSTR::from_raw(path.as_mut_ptr()),
            None,
            None,
            None,
            false,
            DEBUG_ONLY_THIS_PROCESS,
            None,
            None,
            &startupinfo,
            &mut process_info,
        )
        .context(WindowsOpSnafu {
            op: "create QQ process with debugging",
        })?;
    }

    Ok(process_info)
}

/// Handles an exception debug event.
fn handle_exception_event(
    debug_event: &DEBUG_EVENT,
    breakpoint: &mut Option<SoftwareBreakpoint>,
    stepping_threads: &mut HashMap<u32, ThreadStepState>,
    h_process: HANDLE,
) -> Result<ExceptionResult> {
    let exception = unsafe { debug_event.u.Exception };
    let thread_id = debug_event.dwThreadId;
    let exception_code = exception.ExceptionRecord.ExceptionCode;
    let exception_address = exception.ExceptionRecord.ExceptionAddress as u64;

    if exception_code == EXCEPTION_BREAKPOINT {
        handle_breakpoint(
            breakpoint,
            stepping_threads,
            h_process,
            thread_id,
            exception_address,
        )
    } else if exception_code == EXCEPTION_SINGLE_STEP {
        handle_single_step(breakpoint, stepping_threads, thread_id, h_process)
    } else {
        Ok(ExceptionResult::Continue(DBG_EXCEPTION_NOT_HANDLED))
    }
}

/// Handles a software breakpoint exception.
fn handle_breakpoint(
    breakpoint: &mut Option<SoftwareBreakpoint>,
    stepping_threads: &mut HashMap<u32, ThreadStepState>,
    h_process: HANDLE,
    thread_id: u32,
    exception_address: u64,
) -> Result<ExceptionResult> {
    let Some(bp) = breakpoint.as_mut() else {
        return Ok(ExceptionResult::Continue(DBG_CONTINUE));
    };

    if !bp.is_active || exception_address != bp.address {
        return Ok(ExceptionResult::Continue(DBG_CONTINUE));
    }

    debug!("Software breakpoint hit at 0x{:X}", exception_address);

    // Restore original byte
    if let Err(e) = write_remote_memory(h_process, bp.address, &[bp.original_byte]) {
        error!("Failed to restore original byte: {}", e);
    }

    // Get thread context
    let h_thread = open_thread(thread_id)?;
    let mut ctx = get_thread_context(h_thread.as_raw())?;

    // RIP points to instruction after breakpoint, decrement to point to original
    ctx.Rip -= 1;

    debug!("R8 Register Value: 0x{:X}", ctx.R8);

    let target_ptr = ctx.R8;
    let str_result = read_remote_string(h_process, target_ptr, 256);

    match str_result {
        Ok(ref s) if s.is_ascii() && s.len() == 16 => {
            info!("Found target key: {}", s);

            // Restore breakpoint before termination
            if let Err(e) = write_remote_memory(h_process, bp.address, &[0xCC]) {
                error!("Failed to restore breakpoint before termination: {}", e);
            }

            Ok(ExceptionResult::KeyFound(s.clone()))
        }
        Ok(ref unwanted) => {
            debug!("Non-target call with R8 string: {}", unwanted);

            // Set single-step flag
            ctx.EFlags |= 0x100; // TF flag

            set_thread_context(h_thread.as_raw(), &ctx)?;

            // Save state for restoring breakpoint after step
            stepping_threads.insert(
                thread_id,
                ThreadStepState {
                    breakpoint_address: bp.address,
                },
            );

            bp.is_active = false;

            debug!("Prepared thread {} for single-step", thread_id);
            Ok(ExceptionResult::Continue(DBG_CONTINUE))
        }
        Err(e) => {
            error!("Failed to read R8 as string pointer: {}", e);
            Ok(ExceptionResult::Continue(DBG_CONTINUE))
        }
    }
}

/// Handles a single-step exception.
fn handle_single_step(
    breakpoint: &mut Option<SoftwareBreakpoint>,
    stepping_threads: &mut HashMap<u32, ThreadStepState>,
    thread_id: u32,
    h_process: HANDLE,
) -> Result<ExceptionResult> {
    let Some(thread_state) = stepping_threads.remove(&thread_id) else {
        return Ok(ExceptionResult::Continue(DBG_CONTINUE));
    };

    debug!("Single-step exception in thread {}", thread_id);

    let h_thread = open_thread(thread_id)?;
    let mut ctx = get_thread_context(h_thread.as_raw())?;

    // Clear single-step flag
    ctx.EFlags &= !0x100;
    set_thread_context(h_thread.as_raw(), &ctx)?;

    // Restore breakpoint
    if let Err(e) = write_remote_memory(h_process, thread_state.breakpoint_address, &[0xCC]) {
        error!("Failed to restore breakpoint after step: {}", e);
    } else {
        if let Some(bp) = breakpoint.as_mut() {
            bp.is_active = true;
        }
        debug!(
            "Breakpoint restored at 0x{:X}",
            thread_state.breakpoint_address
        );
    }

    Ok(ExceptionResult::Continue(DBG_CONTINUE))
}

/// Opens a thread with full access.
fn open_thread(thread_id: u32) -> Result<OwnedHandle> {
    let handle = unsafe {
        OpenThread(THREAD_ALL_ACCESS, false, thread_id)
            .context(WindowsOpSnafu { op: "open thread" })?
    };
    OwnedHandle::new(handle).context(DebugForKeySnafu {
        msg: "invalid thread handle",
    })
}

/// Gets the thread context.
fn get_thread_context(h_thread: HANDLE) -> Result<CONTEXT> {
    let mut ctx = CONTEXT {
        ContextFlags: CONTEXT_ALL_AMD64,
        ..Default::default()
    };

    unsafe {
        GetThreadContext(h_thread, &mut ctx).context(WindowsOpSnafu {
            op: "get thread context",
        })?;
    }

    Ok(ctx)
}

/// Sets the thread context.
fn set_thread_context(h_thread: HANDLE, ctx: &CONTEXT) -> Result<()> {
    unsafe {
        SetThreadContext(h_thread, ctx).context(WindowsOpSnafu {
            op: "set thread context",
        })?;
    }
    Ok(())
}

/// Terminates a process.
fn terminate_process(h_process: HANDLE) -> Result<()> {
    unsafe {
        TerminateProcess(h_process, 0).context(WindowsOpSnafu {
            op: "terminate QQ process after key extraction",
        })?;
    }
    Ok(())
}

/// Sets a software breakpoint (writes INT3 instruction).
fn set_software_breakpoint(h_process: HANDLE, address: u64) -> Result<u8> {
    // Read original byte
    let mut original_byte = 0u8;
    let mut bytes_read = 0;

    unsafe {
        ReadProcessMemory(
            h_process,
            address as *const c_void,
            std::ptr::from_mut(&mut original_byte).cast(),
            1,
            Some(&mut bytes_read),
        )
        .context(WindowsOpSnafu {
            op: "read original byte for breakpoint",
        })?;
    }

    if bytes_read != 1 {
        return Err(DebugForKeySnafu {
            msg: "read original byte",
        }
        .build());
    }

    // Write INT3 instruction (0xCC)
    let int3_byte = 0xCCu8;
    let mut bytes_written = 0;

    unsafe {
        WriteProcessMemory(
            h_process,
            address as *mut c_void,
            std::ptr::from_ref(&int3_byte).cast(),
            1,
            Some(&mut bytes_written),
        )
        .context(WindowsOpSnafu {
            op: "write INT3 breakpoint",
        })?;
    }

    if bytes_written != 1 {
        return Err(DebugForKeySnafu {
            msg: "write breakpoint",
        }
        .build());
    }

    // Flush instruction cache
    unsafe {
        FlushInstructionCache(h_process, Some(address as *const c_void), 1).context(
            WindowsOpSnafu {
                op: "flush instruction cache",
            },
        )?;
    }

    Ok(original_byte)
}

/// Writes to remote process memory.
fn write_remote_memory(h_process: HANDLE, address: u64, data: &[u8]) -> Result<()> {
    let mut bytes_written = 0;

    unsafe {
        WriteProcessMemory(
            h_process,
            address as *mut c_void,
            data.as_ptr().cast(),
            data.len(),
            Some(&mut bytes_written),
        )
        .context(WindowsOpSnafu {
            op: "write remote memory",
        })?;
    }

    if bytes_written != data.len() {
        return Err(DebugForKeySnafu {
            msg: "write remote memory",
        }
        .build());
    }

    // Flush instruction cache (in case it's code)
    unsafe {
        FlushInstructionCache(h_process, Some(address as *const c_void), data.len()).context(
            WindowsOpSnafu {
                op: "flush instruction cache",
            },
        )?;
    }

    Ok(())
}

/// Gets the base address of a module in a process.
fn get_module_base(pid: u32, name: &str) -> Result<u64> {
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid).context(
            WindowsOpSnafu {
                op: "create toolhelp snapshot",
            },
        )?
    };

    let _snapshot_guard = OwnedHandle::new(snapshot);

    let mut entry = MODULEENTRY32 {
        dwSize: u32::try_from(size_of::<MODULEENTRY32>()).expect("MODULEENTRY32 size fits in u32"),
        ..Default::default()
    };

    unsafe {
        if Module32First(snapshot, &mut entry).is_ok() {
            loop {
                let module_name = CStr::from_ptr(entry.szModule.as_ptr())
                    .to_string_lossy()
                    .to_string();

                if module_name.eq_ignore_ascii_case(name) {
                    return Ok(entry.modBaseAddr as u64);
                }

                if Module32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
    }

    Err(DebugForKeySnafu {
        msg: format!("module '{}' not found", name),
    }
    .build())
}

/// Reads a string from remote process memory.
fn read_remote_string(
    h_process: HANDLE,
    base_addr: u64,
    max_len: usize,
) -> std::result::Result<String, Error> {
    let mut buffer = vec![0u8; max_len];
    let mut bytes_read = 0;

    unsafe {
        ReadProcessMemory(
            h_process,
            base_addr as *const c_void,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            Some(&mut bytes_read),
        )
        .context(WindowsOpSnafu {
            op: "read remote string",
        })?;
    }

    if bytes_read == 0 {
        return Ok(String::new());
    }

    let data = &buffer[..bytes_read];

    // Find null terminator
    if let Some(null_idx) = data.iter().position(|&b| b == 0) {
        let string_data = &data[..null_idx];
        parse_string_data(string_data)
    } else {
        parse_string_data(data)
    }
}

/// Parses byte data into a string.
fn parse_string_data(data: &[u8]) -> std::result::Result<String, Error> {
    if data.iter().all(|&b| b.is_ascii() && !b.is_ascii_control()) {
        String::from_utf8(data.to_vec())
            .ok()
            .context(DebugForKeySnafu {
                msg: "invalid UTF-8 in remote string",
            })
    } else {
        Ok(String::from_utf8_lossy(data).to_string())
    }
}
