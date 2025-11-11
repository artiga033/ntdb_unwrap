//! A SQLite VFS implementation that adds an offset to all file operations.
//! As NTQQ database has a custom file header of 1024 bytes, this VFS is used to skip the header while
//! keeping the original file intact, so as to avoid extra disk op cost.
//!
//! **Warning**: This VFS is specifically designed to read NTQQ database files,
//! any other intended use that out of this project is undefined.

use core::ffi::*;
use core::ptr;
#[cfg(feature = "_cdylib")]
use core::{mem::MaybeUninit, ptr::null};
use libsqlite3_sys::*;

// workaround for no_std
#[allow(non_camel_case_types)]
type sqlite3_filename = *const c_char;

/// theoretically, the slice expect 1024 bytes.  
/// As of now, it works by checking the fixed 8 bytes in [32,40] of the header.
/// So passing a slice of no less than 40 bytes is enough.
///
/// Return:
///   - [Some]: `true`` if the header matches the NTQQ database header, `false`` if not.
///   - [None]: there is no enough information to definitively determine if it's a NTQQ database.
fn match_ntqq_db_header(buf: &[u8]) -> Option<bool> {
    // test fixed 8 bytes in [32,40] of the header
    const NT_QQ_DB_PAT: [u8; 8] = [0x51, 0x51, 0x5f, 0x4e, 0x54, 0x20, 0x44, 0x42];
    buf.get(32..40).map(|x| x == NT_QQ_DB_PAT)
}
#[cfg(feature = "_cdylib")]
static mut SQLITE3_API: MaybeUninit<&sqlite3_api_routines> = MaybeUninit::uninit();
#[inline(always)]
unsafe fn orig_vfs(ptr: *const sqlite3_vfs) -> *mut sqlite3_vfs {
    (*ptr).pAppData as *mut sqlite3_vfs
}
#[inline(always)]
unsafe fn orig_file(ptr: *const sqlite3_file) -> *mut sqlite3_file {
    (&mut (*(ptr as *mut OffsetFile)).origin) as *mut sqlite3_file
}

pub const OFFSET_VFS_NAME: &str = "offset_vfs";
const OFFSET_VFS_NAME_C: &CStr = c"offset_vfs";

#[repr(C)]
struct OffsetFile {
    base: sqlite3_file,
    offset: u64,
    /// this field is of variable length actually, but we only care about the [sqlite3_file] part.  
    /// However this affects the size we need to define in [sqlite3_vfs].
    /// see the `szOsFile`` field of [OFFSET_VFS] and the [register_offset_vfs] function for more details.
    origin: sqlite3_file,
}
impl OffsetFile {
    const OFFSET_AT_ORIGIN: c_int = core::mem::offset_of!(Self, origin) as c_int;
}
static mut OFFSET_VFS: sqlite3_vfs = sqlite3_vfs {
    iVersion: 3,                                        /* iVersion (set when registered) */
    szOsFile: OffsetFile::OFFSET_AT_ORIGIN,             /* szOsFile (set when registered) */
    mxPathname: 1024,                                   /* mxPathname */
    pNext: ptr::null_mut(),                             /* pNext */
    zName: OFFSET_VFS_NAME_C.as_ptr(),                  /* zName */
    pAppData: ptr::null_mut(),                          /* pAppData (set when registered) */
    xOpen: Some(offset_open),                           /* xOpen */
    xDelete: Some(offset_delete),                       /* xDelete */
    xAccess: Some(offset_access),                       /* xAccess */
    xFullPathname: Some(offset_full_pathname),          /* xFullPathname */
    xDlOpen: Some(offset_dl_open),                      /* xDlOpen */
    xDlError: Some(offset_dl_error),                    /* xDlError */
    xDlSym: Some(offset_dl_sym),                        /* xDlSym */
    xDlClose: Some(offset_dl_close),                    /* xDlClose */
    xRandomness: Some(offset_randomness),               /* xRandomness */
    xSleep: Some(offset_sleep),                         /* xSleep */
    xCurrentTime: Some(offset_current_time),            /* xCurrentTime */
    xGetLastError: Some(offset_get_last_error),         /* xGetLastError */
    xCurrentTimeInt64: Some(offset_current_time_int64), /* xCurrentTimeInt64 */
    xGetSystemCall: Some(offset_get_system_call),       /* xSetSystemCall */
    xNextSystemCall: Some(offset_next_system_call),     /* xGetSystemCall */
    xSetSystemCall: Some(offset_set_system_call),       /* xNextSystemCall */
};

static OFFSET_IO_METHODS: sqlite3_io_methods = sqlite3_io_methods {
    iVersion: 3,                                                 /* iVersion */
    xClose: Some(offset_close),                                  /* xClose */
    xRead: Some(offset_read),                                    /* xRead */
    xWrite: Some(offset_write),                                  /* xWrite */
    xTruncate: Some(offset_truncate),                            /* xTruncate */
    xSync: Some(offset_sync),                                    /* xSync */
    xFileSize: Some(offset_file_size),                           /* xFileSize */
    xLock: Some(offset_lock),                                    /* xLock */
    xUnlock: Some(offset_unlock),                                /* xUnlock */
    xCheckReservedLock: Some(offset_check_reserved_lock),        /* xCheckReservedLock */
    xFileControl: Some(offset_file_control),                     /* xFileControl */
    xSectorSize: Some(offset_sector_size),                       /* xSectorSize */
    xDeviceCharacteristics: Some(offset_device_characteristics), /* xDeviceCharacteristics */
    xShmMap: Some(offset_shm_map),                               /* xShmMap */
    xShmLock: Some(offset_shm_lock),                             /* xShmLock */
    xShmBarrier: Some(offset_shm_barrier),                       /* xShmBarrier */
    xShmUnmap: Some(offset_shm_unmap),                           /* xShmUnmap */
    xFetch: Some(offset_fetch),                                  /* xFetch */
    xUnfetch: Some(offset_unfetch),                              /* xUnfetch */
};

unsafe extern "C" fn offset_close(p_file: *mut sqlite3_file) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xClose.unwrap_unchecked()(orig_file)
}
/// Read from the file with offset.
unsafe extern "C" fn offset_read(
    p_file: *mut sqlite3_file,
    p_buf: *mut c_void,
    i_amt: i32,
    i_ofst: sqlite3_int64,
) -> c_int {
    let p_file = p_file as *mut OffsetFile;
    let orig_file = &mut (*p_file).origin;
    (*orig_file.pMethods).xRead.unwrap_unchecked()(
        orig_file,
        p_buf,
        i_amt,
        i_ofst + (*p_file).offset as i64,
    )
}
#[allow(unused_variables)]
unsafe extern "C" fn offset_write(
    p_file: *mut sqlite3_file,
    p_buf: *const c_void,
    i_amt: i32,
    i_ofst: sqlite3_int64,
) -> c_int {
    let p_file = p_file as *mut OffsetFile;
    let orig_file = &mut (*p_file).origin;
    (*orig_file.pMethods).xWrite.unwrap_unchecked()(
        orig_file,
        p_buf,
        i_amt,
        i_ofst + (*p_file).offset as sqlite_int64,
    )
}
#[allow(unused_variables)]
unsafe extern "C" fn offset_truncate(p_file: *mut sqlite3_file, size: sqlite3_int64) -> c_int {
    let p_file = p_file as *mut OffsetFile;
    let orig_file = &mut (*p_file).origin;
    (*orig_file.pMethods).xTruncate.unwrap_unchecked()(
        orig_file,
        size + (*p_file).offset as sqlite_int64,
    )
}
#[allow(unused_variables)]
unsafe extern "C" fn offset_sync(p_file: *mut sqlite3_file, flags: i32) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xSync.unwrap_unchecked()(orig_file, flags)
}
unsafe extern "C" fn offset_file_size(
    p_file: *mut sqlite3_file,
    p_size: *mut sqlite3_int64,
) -> c_int {
    let p_file = p_file as *mut OffsetFile;
    let orig_file = &mut (*p_file).origin;
    let rc = (*orig_file.pMethods).xFileSize.unwrap_unchecked()(orig_file, p_size);
    if rc == SQLITE_OK {
        *p_size -= (*p_file).offset as sqlite3_int64;
    }
    rc
}
unsafe extern "C" fn offset_lock(p_file: *mut sqlite3_file, e_lock: i32) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xLock.unwrap_unchecked()(orig_file, e_lock)
}
unsafe extern "C" fn offset_unlock(p_file: *mut sqlite3_file, e_lock: i32) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xUnlock.unwrap_unchecked()(orig_file, e_lock)
}
unsafe extern "C" fn offset_check_reserved_lock(
    p_file: *mut sqlite3_file,
    p_res_out: *mut i32,
) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods)
        .xCheckReservedLock
        .unwrap_unchecked()(orig_file, p_res_out)
}
unsafe extern "C" fn offset_file_control(
    p_file: *mut sqlite3_file,
    op: i32,
    p_arg: *mut c_void,
) -> c_int {
    let file = p_file as *mut OffsetFile;
    let orig_file = &mut (*file).origin;
    if op == SQLITE_FCNTL_SIZE_HINT {
        let p_arg = p_arg as *mut sqlite3_int64;
        *p_arg += (*file).offset as sqlite3_int64;
    }
    let rc = (*orig_file.pMethods).xFileControl.unwrap_unchecked()(orig_file, op, p_arg);
    if (rc == SQLITE_OK) && (op == SQLITE_FCNTL_VFSNAME) {
        let p_arg = p_arg as *mut *mut c_char;
        *p_arg = {
            #[cfg(not(feature = "_cdylib"))]
            {
                sqlite3_mprintf(c"offset(%s)/%z".as_ptr(), (*file).offset, p_arg)
            }
            #[cfg(feature = "_cdylib")]
            {
                SQLITE3_API.assume_init().mprintf.unwrap_unchecked()(
                    c"offset(%s)/%z".as_ptr(),
                    (*file).offset,
                    p_arg,
                )
            }
        }
    }
    rc
}
unsafe extern "C" fn offset_sector_size(p_file: *mut sqlite3_file) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xSectorSize.unwrap_unchecked()(orig_file)
}
unsafe extern "C" fn offset_device_characteristics(p_file: *mut sqlite3_file) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods)
        .xDeviceCharacteristics
        .unwrap_unchecked()(orig_file)
}
unsafe extern "C" fn offset_shm_map(
    p_file: *mut sqlite3_file,
    i_pg: i32,
    pgsz: i32,
    flags: i32,
    pp: *mut *mut c_void,
) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xShmMap.unwrap_unchecked()(orig_file, i_pg, pgsz, flags, pp)
}

unsafe extern "C" fn offset_shm_lock(
    p_file: *mut sqlite3_file,
    offset: i32,
    n: i32,
    flags: i32,
) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xShmLock.unwrap_unchecked()(orig_file, offset, n, flags)
}

unsafe extern "C" fn offset_shm_barrier(p_file: *mut sqlite3_file) {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xShmBarrier.unwrap_unchecked()(orig_file)
}

unsafe extern "C" fn offset_shm_unmap(p_file: *mut sqlite3_file, delete_flag: i32) -> c_int {
    let orig_file = orig_file(p_file);
    (*(*orig_file).pMethods).xShmUnmap.unwrap_unchecked()(orig_file, delete_flag)
}

unsafe extern "C" fn offset_fetch(
    p_file: *mut sqlite3_file,
    i_ofst: sqlite3_int64,
    i_amt: i32,
    pp: *mut *mut c_void,
) -> c_int {
    let file = p_file as *mut OffsetFile;
    let orig_file = &mut (*file).origin;
    (*orig_file.pMethods).xFetch.unwrap_unchecked()(
        orig_file,
        i_ofst + (*file).offset as sqlite_int64,
        i_amt,
        pp,
    )
}

unsafe extern "C" fn offset_unfetch(
    p_file: *mut sqlite3_file,
    i_ofst: sqlite3_int64,
    p: *mut c_void,
) -> c_int {
    let file = p_file as *mut OffsetFile;
    let orig_file = &mut (*file).origin;
    (*orig_file.pMethods).xUnfetch.unwrap_unchecked()(
        orig_file,
        i_ofst + (*file).offset as sqlite_int64,
        p,
    )
}

/// Open a file using the underlying VFS and set the offset accordingly,
/// which is 1024 for NTQQ database.
unsafe extern "C" fn offset_open(
    vfs: *mut sqlite3_vfs,
    z_name: sqlite3_filename,
    p_file: *mut sqlite3_file,
    flags: c_int,
    p_out_flags: *mut c_int,
) -> c_int {
    let file = p_file as *mut OffsetFile;
    let base_vfs = orig_vfs(vfs);
    let base_file = orig_file(p_file);

    // there's only offset on main db
    if (flags & SQLITE_OPEN_MAIN_DB) == 0 {
        // use our p_file as the underlying type, it's definitely long enough, after this we no longer take care of it
        return (*base_vfs).xOpen.unwrap_unchecked()(base_vfs, z_name, p_file, flags, p_out_flags);
    }

    *base_file = core::mem::zeroed();
    let rc: c_int =
        (*base_vfs).xOpen.unwrap_unchecked()(base_vfs, z_name, base_file, flags, p_out_flags);
    if rc != SQLITE_OK {
        return rc;
    }
    (*p_file).pMethods = &OFFSET_IO_METHODS;

    let mut buf = [0u8; 1024];
    (*base_file).pMethods.as_ref().unwrap().xRead.unwrap()(
        base_file,
        buf.as_mut_ptr() as *mut c_void,
        1024,
        0,
    );
    // fixed offset of 1024
    (*file).offset = if let Some(true) = match_ntqq_db_header(&buf) {
        1024
    } else {
        0
    };
    rc
}

#[allow(unused_variables)]
/// Delete is not supported, always error.
unsafe extern "C" fn offset_delete(
    arg1: *mut sqlite3_vfs,
    z_name: sqlite3_filename,
    sync_dir: i32,
) -> i32 {
    (*orig_vfs(arg1)).xDelete.unwrap_unchecked()(orig_vfs(arg1), z_name, sync_dir)
}
// all other methods are just pass-through
unsafe extern "C" fn offset_access(
    arg1: *mut sqlite3_vfs,
    z_name: sqlite3_filename,
    flags: i32,
    p_res_out: *mut i32,
) -> i32 {
    (*orig_vfs(arg1)).xAccess.unwrap_unchecked()(orig_vfs(arg1), z_name, flags, p_res_out)
}
unsafe extern "C" fn offset_full_pathname(
    arg1: *mut sqlite3_vfs,
    z_name: *const c_char,
    n_out: i32,
    z_out: *mut c_char,
) -> i32 {
    (*orig_vfs(arg1)).xFullPathname.unwrap_unchecked()(orig_vfs(arg1), z_name, n_out, z_out)
}
unsafe extern "C" fn offset_dl_open(
    arg1: *mut sqlite3_vfs,
    z_filename: *const c_char,
) -> *mut c_void {
    (*orig_vfs(arg1)).xDlOpen.unwrap_unchecked()(orig_vfs(arg1), z_filename)
}
unsafe extern "C" fn offset_dl_error(arg1: *mut sqlite3_vfs, n_byte: i32, z_err_msg: *mut c_char) {
    (*orig_vfs(arg1)).xDlError.unwrap_unchecked()(orig_vfs(arg1), n_byte, z_err_msg)
}
unsafe extern "C" fn offset_dl_sym(
    arg1: *mut sqlite3_vfs,
    arg2: *mut c_void,
    z_symbol: *const c_char,
) -> Option<unsafe extern "C" fn(arg1: *mut sqlite3_vfs, arg2: *mut c_void, z_symbol: *const c_char)>
{
    (*orig_vfs(arg1)).xDlSym.unwrap_unchecked()(orig_vfs(arg1), arg2, z_symbol)
}
unsafe extern "C" fn offset_dl_close(arg1: *mut sqlite3_vfs, arg2: *mut c_void) {
    (*orig_vfs(arg1)).xDlClose.unwrap_unchecked()(orig_vfs(arg1), arg2)
}
unsafe extern "C" fn offset_randomness(
    arg1: *mut sqlite3_vfs,
    n_byte: i32,
    z_out: *mut c_char,
) -> i32 {
    (*orig_vfs(arg1)).xRandomness.unwrap_unchecked()(orig_vfs(arg1), n_byte, z_out)
}
unsafe extern "C" fn offset_sleep(arg1: *mut sqlite3_vfs, microseconds: i32) -> i32 {
    (*orig_vfs(arg1)).xSleep.unwrap_unchecked()(orig_vfs(arg1), microseconds)
}
unsafe extern "C" fn offset_current_time(arg1: *mut sqlite3_vfs, arg2: *mut f64) -> i32 {
    (*orig_vfs(arg1)).xCurrentTime.unwrap_unchecked()(orig_vfs(arg1), arg2)
}
unsafe extern "C" fn offset_get_last_error(
    arg1: *mut sqlite3_vfs,
    arg2: i32,
    arg3: *mut c_char,
) -> i32 {
    (*orig_vfs(arg1)).xGetLastError.unwrap_unchecked()(orig_vfs(arg1), arg2, arg3)
}
unsafe extern "C" fn offset_current_time_int64(
    arg1: *mut sqlite3_vfs,
    arg2: *mut sqlite3_int64,
) -> c_int {
    (*orig_vfs(arg1)).xCurrentTimeInt64.unwrap_unchecked()(orig_vfs(arg1), arg2)
}
unsafe extern "C" fn offset_get_system_call(
    arg1: *mut sqlite3_vfs,
    z_name: *const c_char,
) -> sqlite3_syscall_ptr {
    (*orig_vfs(arg1)).xGetSystemCall.unwrap_unchecked()(orig_vfs(arg1), z_name)
}
unsafe extern "C" fn offset_next_system_call(
    arg1: *mut sqlite3_vfs,
    z_name: *const c_char,
) -> *const c_char {
    (*orig_vfs(arg1)).xNextSystemCall.unwrap_unchecked()(orig_vfs(arg1), z_name)
}
unsafe extern "C" fn offset_set_system_call(
    arg1: *mut sqlite3_vfs,
    z_name: *const c_char,
    arg2: sqlite3_syscall_ptr,
) -> i32 {
    (*orig_vfs(arg1)).xSetSystemCall.unwrap_unchecked()(orig_vfs(arg1), z_name, arg2)
}

#[inline(always)]
unsafe fn update_vfs_from_orig(origin: *mut sqlite3_vfs) {
    // already registered, don't update
    // or will cause an infinite loop
    if (*origin).zName == OFFSET_VFS_NAME_C.as_ptr() {
        return;
    }
    OFFSET_VFS.iVersion = (*origin).iVersion;
    OFFSET_VFS.pAppData = origin as *mut c_void;
    OFFSET_VFS.szOsFile += (*origin).szOsFile;
}

/// Entry point for SQLite to load the extension as a C dynamic library.
#[allow(clippy::missing_safety_doc)]
#[cfg(feature = "_cdylib")]
#[no_mangle]
pub unsafe extern "C" fn sqlite3_sqliteextntqqdb_init(
    _db: *mut sqlite3,
    _pz_err_msg: *mut *mut c_char,
    p_api: *mut sqlite3_api_routines,
) -> c_int {
    let p_api = &mut *p_api;
    SQLITE3_API = MaybeUninit::new(p_api);
    let origin = SQLITE3_API.assume_init().vfs_find.unwrap_unchecked()(null());
    update_vfs_from_orig(origin);
    match SQLITE3_API.assume_init().vfs_register.unwrap_unchecked()(&raw mut OFFSET_VFS, 1) {
        SQLITE_OK => SQLITE_OK_LOAD_PERMANENTLY,
        rc => rc,
    }
}

/// working as a rust library, providing Rusty API.
#[cfg(not(feature = "_cdylib"))]
mod rlib {
    use core::ptr::null;

    use super::*;

    /// Register the offset VFS to SQLite.
    pub fn register_offset_vfs() -> Result<(), i32> {
        match unsafe {
            let origin = sqlite3_vfs_find(null());
            update_vfs_from_orig(origin);
            sqlite3_vfs_register(&raw mut OFFSET_VFS, 1)
        } {
            0 => Ok(()),
            rc => Err(rc),
        }
    }
}
#[cfg(not(feature = "_cdylib"))]
pub use rlib::*;
