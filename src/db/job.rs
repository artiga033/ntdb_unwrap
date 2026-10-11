//! Kill private readers when the parent exits, including console cancellation.
use std::{
    os::windows::io::AsRawHandle,
    process::Child,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::*,
};
static JOBS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
static CANCELLED: AtomicBool = AtomicBool::new(false);
pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}
pub struct Cancellation;
unsafe extern "system" fn on_control(code: u32) -> windows::core::BOOL {
    use windows::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if code != CTRL_C_EVENT && code != CTRL_BREAK_EVENT {
        return false.into();
    }
    CANCELLED.store(true, Ordering::SeqCst);
    if let Ok(jobs) = JOBS.lock() {
        for handle in jobs.iter() {
            unsafe {
                let _ = TerminateJobObject(HANDLE(*handle as *mut _), 1);
            }
        }
    }
    true.into()
}
pub fn cancellation() -> windows::core::Result<Cancellation> {
    unsafe {
        windows::Win32::System::Console::SetConsoleCtrlHandler(Some(on_control), true)?;
    }
    Ok(Cancellation)
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Console::SetConsoleCtrlHandler(Some(on_control), false);
        }
    }
}
pub struct Job(HANDLE);
impl Job {
    pub fn assign(child: &Child) -> windows::core::Result<Self> {
        unsafe {
            let job = Self(CreateJobObjectW(None, None)?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )?;
            AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle()))?;
            JOBS.lock().unwrap().push(job.0.0 as isize);
            if cancelled() {
                let _ = TerminateJobObject(job.0, 1);
            }
            Ok(job)
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        let mut jobs = JOBS.lock().unwrap();
        jobs.retain(|h| *h != self.0.0 as isize);
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
