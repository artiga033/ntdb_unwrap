//! Private self-process reader. Secrets travel exclusively over stdin.
use crate::{Error, Result};
use ntdb_unwrap::{
    db::{
        self,
        stream::{self, Decoder, Event},
    },
    ntqq::DBDecryptInfo,
};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
};
fn app_error(e: impl std::fmt::Display) -> Error {
    Error::App {
        message: e.to_string(),
    }
}
#[derive(Clone)]
pub struct Config {
    pub path: PathBuf,
    pub key: String,
    pub algorithm: Option<String>,
    pub offset: Option<i64>,
    pub diagnostic: bool,
}
impl Config {
    pub fn from_prepared(p: &super::common::Prepared) -> Self {
        Self {
            path: p.user_db_file.path.clone(),
            key: p.decrypt_info.key.clone(),
            algorithm: p.decrypt_info.cipher_hmac_algorithm.clone(),
            offset: p.offset,
            diagnostic: false,
        }
    }
    fn encode(&self) -> stream::Result<Vec<u8>> {
        let mut b = vec![u8::from(self.diagnostic)];
        // Paths are encoded losslessly, including Windows non-Unicode filenames.
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let bytes: Vec<u8> = self
                .path
                .as_os_str()
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect();
            stream::put_bytes(&mut b, &bytes);
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::ffi::OsStrExt;
            stream::put_bytes(&mut b, self.path.as_os_str().as_bytes());
        }
        stream::put_bytes(&mut b, self.key.as_bytes());
        b.push(u8::from(self.algorithm.is_some()));
        if let Some(a) = &self.algorithm {
            stream::put_bytes(&mut b, a.as_bytes());
        }
        b.push(u8::from(self.offset.is_some()));
        if let Some(o) = self.offset {
            b.extend_from_slice(&o.to_le_bytes());
        }
        Ok(b)
    }
    fn decode(b: &[u8]) -> stream::Result<Self> {
        let mut d = Decoder(b);
        let diagnostic = match d.byte()? {
            0 => false,
            1 => true,
            _ => return Err("invalid reader task".into()),
        };
        let bytes = d.bytes()?;
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStringExt;
            if bytes.len() % 2 != 0 {
                return Err("invalid path encoding".into());
            }
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            PathBuf::from(std::ffi::OsString::from_wide(&units))
        };
        #[cfg(not(windows))]
        let path = {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(bytes))
        };
        let key = d.string()?;
        let algorithm = match d.byte()? {
            0 => None,
            1 => Some(d.string()?),
            _ => return Err("invalid optional algorithm".into()),
        };
        let offset = match d.byte()? {
            0 => None,
            1 => Some(d.integer()?),
            _ => return Err("invalid optional offset".into()),
        };
        d.end()?;
        Ok(Self {
            path,
            key,
            algorithm,
            offset,
            diagnostic,
        })
    }
}
pub struct Reader {
    child: Child,
    pub stdout: BufReader<std::process::ChildStdout>,
    #[cfg(windows)]
    _job: db::job::Job,
}
impl Reader {
    pub fn spawn(config: &Config) -> stream::Result<Self> {
        #[cfg(windows)]
        if db::job::cancelled() {
            return Err("任务已取消".into());
        }
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("__internal-db-reader")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        #[cfg(windows)]
        let job = match db::job::Job::assign(&child) {
            Ok(j) => j,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.into());
            }
        };
        let result = (|| {
            let mut stdin = child.stdin.take().ok_or("missing reader stdin")?;
            stream::write_frame(&mut stdin, &config.encode()?)?;
            stdin.flush()?;
            drop(stdin);
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })();
        if let Err(e) = result {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        let stdout = BufReader::new(child.stdout.take().ok_or("missing reader stdout")?);
        Ok(Self {
            child,
            stdout,
            #[cfg(windows)]
            _job: job,
        })
    }
    pub fn finish(&mut self) -> stream::Result<()> {
        let mut byte = [0; 1];
        if self.stdout.read(&mut byte)? != 0 {
            return Err("reader sent trailing data".into());
        }
        let status = self.child.wait()?;
        if !status.success() {
            return Err(format!("reader process failed: {status}").into());
        }
        Ok(())
    }
    pub fn consume(&mut self, conn: &mut rusqlite::Connection) -> stream::Result<()> {
        // RefCell permits the two sequential callbacks to borrow the same process.
        let reader = std::cell::RefCell::new(self);
        stream::consume(
            conn,
            || stream::read_event(&mut reader.borrow_mut().stdout),
            || reader.borrow_mut().finish(),
        )
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub fn internal() -> Result<()> {
    #[cfg(unix)]
    let parent = unsafe { getppid() };
    let mut stdout = BufWriter::new(std::io::stdout().lock());
    let result = (|| -> stream::Result<()> {
        let mut input = std::io::stdin().lock();
        // Configuration is small; reject oversized frames before allocating.
        let mut version = [0; 2];
        input.read_exact(&mut version)?;
        if u16::from_le_bytes(version) != stream::VERSION {
            return Err("unsupported reader protocol version".into());
        }
        let mut length = [0; 4];
        input.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        if length > 1024 * 1024 {
            return Err("reader configuration too large".into());
        }
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes)?;
        let config = Config::decode(&bytes)?;
        let mut extra = [0; 1];
        if input.read(&mut extra)? != 0 {
            return Err("trailing reader configuration".into());
        }
        drop(input);
        unsafe {
            db::pending::configure(config.offset)?;
        }
        let conn = super::common::open_source(
            &config.path,
            &DBDecryptInfo {
                key: config.key,
                cipher_hmac_algorithm: config.algorithm,
            },
        )?;
        #[cfg(unix)]
        install_parent_watch(&conn, parent);
        if config.diagnostic {
            if config.offset.is_none() {
                stream::check(&conn, "quick_check")?;
            } else {
                stream::integrity(&conn)?;
            }
            stream::write_event(&mut stdout, &Event::Done)?;
        } else {
            stream::produce(&conn, |event| stream::write_event(&mut stdout, &event))?;
        }
        stdout.flush()?;
        Ok(())
    })();
    if let Err(e) = result {
        let message = e.to_string();
        let _ = stream::write_event(&mut stdout, &Event::Error(message.clone()));
        let _ = stdout.flush();
        return Err(app_error(message));
    }
    Ok(())
}
#[cfg(unix)]
unsafe extern "C" {
    fn getppid() -> i32;
}
#[cfg(unix)]
fn install_parent_watch(conn: &rusqlite::Connection, parent: i32) {
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    unsafe extern "C" {
        fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
    }
    unsafe extern "C" fn check(data: *mut std::ffi::c_void) -> i32 {
        if unsafe { getppid() } != data as isize as i32 {
            return 1;
        }
        let mut fd = PollFd {
            fd: 1,
            events: 4,
            revents: 0,
        };
        // POLLERR/POLLHUP indicate a closed stdout consumer; lack of POLLOUT
        // alone is normal pipe backpressure and must not cancel the reader.
        unsafe {
            poll(&mut fd, 1, 0);
        }
        i32::from(fd.revents & (8 | 16) != 0)
    }
    unsafe {
        rusqlite::ffi::sqlite3_progress_handler(
            conn.handle(),
            10000,
            Some(check),
            parent as isize as *mut _,
        );
    }
}
/// Only confirm a layout mismatch after specific reserved-page evidence AND a
/// successful isolated full check. A failure leaves the original error intact.
pub fn diagnose(config: &Config, original_error: &str) -> Option<String> {
    if config.offset.is_some()
        || !(original_error.contains("malformed") || original_error.contains("SQLITE_CORRUPT"))
    {
        return None;
    }
    let evidence = (|| -> stream::Result<bool> {
        use std::io::{Seek, SeekFrom};
        let mut file = std::fs::File::open(&config.path)?;
        let mut header = [0; 1024];
        file.read_exact(&mut header)?;
        let header_offset = if &header[32..40] == b"QQ_NT DB" {
            1024u64
        } else {
            0
        };
        let page_size = if &header[..16] == b"SQLite format 3\0" {
            let n = u16::from_be_bytes([header[16], header[17]]);
            if n == 1 { 65536 } else { u64::from(n) }
        } else if header_offset == 1024 {
            4096
        } else {
            return Ok(false);
        };
        if page_size < 512 || !page_size.is_power_of_two() {
            return Ok(false);
        }
        let early = (u64::from(db::pending::STANDARD) - 1024) / page_size + 1;
        let standard = u64::from(db::pending::STANDARD) / page_size + 1;
        if early == standard || file.metadata()?.len() < header_offset + standard * page_size {
            return Ok(false);
        }
        file.seek(SeekFrom::Start(header_offset + (early - 1) * page_size))?;
        let mut page = vec![0; page_size as usize];
        file.read_exact(&mut page)?;
        if page.iter().any(|b| *b != 0) {
            return Ok(false);
        }
        let matches_signature = |text: &str| {
            text.contains(&format!("Page {early}: never used"))
                && text.contains(&format!("2nd reference to page {standard}"))
        };
        if matches_signature(original_error) {
            return Ok(true);
        }
        // Gather the fixed signature with a standard quick_check if the original
        // read failed before SQLite could report its complete integrity results.
        let mut probe = config.clone();
        probe.diagnostic = true;
        let mut child = Reader::spawn(&probe)?;
        let signature = match stream::read_event(&mut child.stdout)? {
            Event::Error(e) => e,
            _ => return Ok(false),
        };
        Ok(matches_signature(&signature))
    })();
    if !matches!(evidence, Ok(true)) {
        return None;
    }
    let mut probe = config.clone();
    probe.diagnostic = true;
    probe.offset = Some(-1024);
    let confirmed = (|| -> stream::Result<()> {
        let mut child = Reader::spawn(&probe)?;
        if stream::read_event(&mut child.stdout)? != Event::Done {
            return Err("diagnostic did not complete".into());
        }
        child.finish()
    })();
    if confirmed.is_ok() {
        let message="已确认源数据库保留页布局与标准 SQLite 不兼容；隔离完整 integrity_check 通过。请手动添加 --pending-byte-offset=-1024 后重试。".to_string();
        Some(message)
    } else {
        None
    }
}
