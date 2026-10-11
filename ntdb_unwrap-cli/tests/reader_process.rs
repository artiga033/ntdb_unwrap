use ntdb_unwrap::{
    db::stream::{self, Event},
    ntqq::DBDecryptInfo,
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::current_dir().unwrap().join(".tmp");
        fs::create_dir_all(&root).unwrap();
        let dir = root.join(format!(
            "process-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn source(&self) -> PathBuf {
        let path = self.0.join("source.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        let info = DBDecryptInfo {
            key: "test secret".into(),
            cipher_hmac_algorithm: Some("HMAC_SHA1".into()),
        };
        conn.execute_batch(&info.display_pragma_stmts().to_string())
            .unwrap();
        conn.execute_batch("CREATE TABLE t(\"40001\" TEXT,\"40002\" BLOB);INSERT INTO t(rowid,\"40001\",\"40002\") VALUES(71,'value',x'00ff');PRAGMA user_version=17").unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = std::env::current_dir().unwrap().join(".tmp");
        assert_eq!(self.0.parent(), Some(root.as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ntdb_unwrap"))
}
#[test]
fn exports_encrypted_source_with_isolated_reader() {
    let f = Fixture::new();
    let source = f.source();
    let before = fs::read(&source).unwrap();
    for (label, offset) in [
        ("default", None),
        ("standard", Some("0")),
        ("positive", Some("1024")),
        ("negative", Some("-1024")),
    ] {
        let out = f.0.join(format!("{label}.db"));
        let mut command = cli();
        command
            .arg("export")
            .arg(&source)
            .args(["-p", "test secret", "-o"])
            .arg(&out)
            .env("TMP", &f.0)
            .env("TEMP", &f.0)
            .env("TMPDIR", &f.0);
        if label != "default" {
            command.arg("-N");
        }
        if let Some(value) = offset {
            command.args(["--pending-byte-offset", value]);
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!String::from_utf8_lossy(&result.stderr).contains("test secret"));
        if label == "default" {
            assert!(String::from_utf8_lossy(&result.stdout).contains("nt_msg_temp_copy.db"));
            assert!(String::from_utf8_lossy(&result.stdout).contains("清理临时文件"));
            assert!(!f.0.join("nt_msg_temp_copy.db").exists());
        }
        let target = rusqlite::Connection::open_with_flags_and_vfs(
            out,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            stream::native_vfs(),
        )
        .unwrap();
        stream::integrity(&target).unwrap();
        assert_eq!(
            target
                .query_row("SELECT rowid FROM t", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            71
        );
        assert_eq!(
            target
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            17
        );
    }
    assert_eq!(fs::read(source).unwrap(), before);
}
#[test]
fn wrong_key_existing_output_and_bad_offset_do_not_publish() {
    let f = Fixture::new();
    let source = f.source();
    let out = f.0.join("output.db");
    let result = cli()
        .arg("export")
        .arg(&source)
        .args(["-p", "wrong secret", "-o"])
        .env("TMP", &f.0)
        .env("TEMP", &f.0)
        .env("TMPDIR", &f.0)
        .arg(&out)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!out.exists());
    let text = String::from_utf8_lossy(&result.stderr);
    assert!(!text.contains("wrong secret"));
    assert!(!text.contains("已确认"));
    assert!(text.contains("WrongDecryptInfo"));
    fs::write(&out, b"unchanged").unwrap();
    assert!(
        !cli()
            .arg("export")
            .arg(&source)
            .args(["-p", "test secret", "-o"])
            .arg(&out)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read(&out).unwrap(), b"unchanged");
    for value in ["-1073741824", "3221225472"] {
        let result = cli()
            .arg("export")
            .arg(&source)
            .args(["-p", "test secret", "--pending-byte-offset", value, "-o"])
            .arg(f.0.join("invalid.db"))
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("1..=u32::MAX"));
    }
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 2);
}
#[test]
fn internal_reader_rejects_versions_and_truncated_configuration() {
    for bytes in [vec![9, 0, 0, 0, 0, 0], vec![1, 0, 10, 0, 0, 0, 0]] {
        let mut child = cli()
            .arg("__internal-db-reader")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&bytes).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(!result.status.success());
        assert!(matches!(
            stream::read_event(&mut &result.stdout[..]).unwrap(),
            Event::Error(_)
        ));
    }
}
#[test]
fn genuine_corruption_does_not_claim_layout_mismatch() {
    let f = Fixture::new();
    let path = f.source();
    let out = f.0.join("out.db");
    let mut bytes = fs::read(&path).unwrap();
    bytes[4096] ^= 1; // Corrupt the encrypted table page; leave its schema readable.
    fs::write(&path, bytes).unwrap();
    let result = cli()
        .arg("export")
        .arg(path)
        .args(["-p", "test secret", "-N", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!out.exists());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("已确认"));
}
