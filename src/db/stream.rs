//! Bounded, synchronous logical export. TEXT bytes are preserved without UTF-8 conversion.
use rusqlite::{
    Connection, OpenFlags,
    types::{ToSql, ToSqlOutput, ValueRef},
};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const VERSION: u16 = 1;
pub const MAX_FRAME: usize = 1_000_000_000;
#[derive(Debug, PartialEq)]
pub enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
impl ToSql for Cell {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            Self::Null => ValueRef::Null,
            Self::Integer(v) => ValueRef::Integer(*v),
            Self::Real(v) => ValueRef::Real(*v),
            Self::Text(v) => ValueRef::Text(v),
            Self::Blob(v) => ValueRef::Blob(v),
        }))
    }
}
#[derive(Debug, PartialEq)]
pub enum Event {
    Metadata(i64, i64),
    Schema {
        kind: String,
        name: String,
        sql: String,
    },
    Table {
        name: String,
        columns: Vec<String>,
    },
    Row(Vec<Cell>),
    End,
    Done,
    Error(String),
}
pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
pub fn integrity(conn: &Connection) -> Result<()> {
    check(conn, "integrity_check")
}
/// Quick checks are used only to gather reserved-page evidence under standard rules.
pub fn check(conn: &Connection, pragma: &str) -> Result<()> {
    if !["integrity_check", "quick_check"].contains(&pragma) {
        return Err("invalid check".into());
    }
    let mut stmt = conn.prepare(&format!("PRAGMA {pragma}"))?;
    let mut rows = stmt.query([])?;
    let mut count = 0;
    let mut problems = Vec::new();
    loop {
        match rows.next() {
            Ok(Some(row)) => {
                count += 1;
                let value: String = row.get(0)?;
                if value != "ok" && problems.len() < 100 {
                    problems.push(value);
                }
            }
            Ok(None) => break,
            Err(e) => {
                if problems.is_empty() {
                    return Err(e.into());
                }
                problems.push(e.to_string());
                break;
            }
        }
    }
    if count != 1 || !problems.is_empty() {
        return Err(format!(
            "database disk image is malformed: {pragma}: {}",
            problems.join("; ")
        )
        .into());
    }
    Ok(())
}
/// Produce one read-transaction snapshot. Sink errors stop traversal immediately.
pub fn produce(conn: &Connection, mut emit: impl FnMut(Event) -> Result<()>) -> Result<()> {
    conn.execute_batch("BEGIN")?;
    let result = (|| {
        // Check the source too: a scan alone can silently skip an incompatible reserved page.
        log::info!("阶段：源数据库完整性检查");
        integrity(conn)?;
        emit(Event::Metadata(
            conn.query_row("PRAGMA user_version", [], |r| r.get(0))?,
            conn.query_row("PRAGMA application_id", [], |r| r.get(0))?,
        ))?;
        let schema = conn
            .prepare(
                "SELECT type,name,sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY rowid",
            )?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Reject virtual tables before emitting any data, including their shadow tables.
        for (kind, name, sql) in &schema {
            if kind == "table"
                && sql
                    .trim_start()
                    .to_ascii_uppercase()
                    .starts_with("CREATE VIRTUAL TABLE")
            {
                return Err(format!(
                    "cannot reliably reconstruct virtual table {name}: extension required"
                )
                .into());
            }
        }
        for (kind, name, sql) in &schema {
            if kind == "table" && !name.starts_with("sqlite_") {
                emit(Event::Schema {
                    kind: kind.clone(),
                    name: name.clone(),
                    sql: sql.clone(),
                })?;
            }
        }
        let sequence_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='sqlite_sequence')",
            [],
            |r| r.get(0),
        )?;
        let mut tables: Vec<String> = schema
            .iter()
            .filter(|(k, n, _)| k == "table" && !n.starts_with("sqlite_"))
            .map(|(_, n, _)| n.clone())
            .collect();
        if sequence_exists {
            tables.push("sqlite_sequence".into());
        }
        for name in tables {
            log::info!("阶段：读取表 {name}");
            let columns = conn
                .prepare(&format!("PRAGMA table_xinfo({})", quote(&name)))?
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(6)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut writable: Vec<String> = columns
                .iter()
                .filter(|(_, h)| *h == 0)
                .map(|(n, _)| n.clone())
                .collect();
            // Probe unshadowed aliases; WITHOUT ROWID naturally rejects all of them.
            for alias in ["rowid", "_rowid_", "oid"] {
                if !columns.iter().any(|(n, _)| n.eq_ignore_ascii_case(alias))
                    && conn
                        .prepare(&format!("SELECT {alias} FROM {} LIMIT 0", quote(&name)))
                        .is_ok()
                {
                    writable.insert(0, alias.into());
                    break;
                }
            }
            if writable.is_empty() {
                return Err(format!("table {name} has no writable columns").into());
            }
            let sql = format!(
                "SELECT {} FROM {} NOT INDEXED",
                writable
                    .iter()
                    .map(|n| quote(n))
                    .collect::<Vec<_>>()
                    .join(","),
                quote(&name)
            );
            emit(Event::Table {
                name,
                columns: writable.clone(),
            })?;
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let mut cells = Vec::with_capacity(writable.len());
                for i in 0..writable.len() {
                    cells.push(match row.get_ref(i)? {
                        ValueRef::Null => Cell::Null,
                        ValueRef::Integer(v) => Cell::Integer(v),
                        ValueRef::Real(v) => Cell::Real(v),
                        ValueRef::Text(v) => Cell::Text(v.to_vec()),
                        ValueRef::Blob(v) => Cell::Blob(v.to_vec()),
                    });
                }
                emit(Event::Row(cells))?;
            }
            emit(Event::End)?;
        }
        for (kind, name, sql) in schema {
            if ["index", "view", "trigger"].contains(&kind.as_str()) && !name.starts_with("sqlite_")
            {
                emit(Event::Schema { kind, name, sql })?;
            }
        }
        emit(Event::Done)
    })();
    let rollback = conn.execute_batch("ROLLBACK");
    result?;
    rollback?;
    Ok(())
}
/// Synchronous sink reusable by the in-process producer and the wire consumer.
pub struct Consumer<'a> {
    tx: rusqlite::Transaction<'a>,
    started: bool,
    data_started: bool,
    deferred: bool,
    done: bool,
    tables: std::collections::HashSet<String>,
    copied: std::collections::HashSet<String>,
    active: Option<(String, usize)>,
    views: Vec<String>,
}
impl<'a> Consumer<'a> {
    pub fn new(conn: &'a mut Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys=OFF")?;
        Ok(Self {
            tx: conn.transaction()?,
            started: false,
            data_started: false,
            deferred: false,
            done: false,
            tables: Default::default(),
            copied: Default::default(),
            active: None,
            views: Vec::new(),
        })
    }
    pub fn accept(&mut self, event: Event) -> Result<()> {
        if self.done {
            return Err("event after completion".into());
        }
        if let Event::Error(e) = event {
            return Err(e.into());
        }
        if let Some((sql, count)) = &self.active {
            match event {
                Event::Row(cells) if cells.len() == *count => {
                    self.tx
                        .prepare_cached(sql)?
                        .execute(rusqlite::params_from_iter(cells.iter()))?;
                    return Ok(());
                }
                Event::End => {
                    self.active = None;
                    return Ok(());
                }
                _ => return Err("invalid table record or truncated stream".into()),
            }
        }
        match event {
            Event::Metadata(u, a) if !self.started => {
                self.tx.pragma_update(None, "user_version", u)?;
                self.tx.pragma_update(None, "application_id", a)?;
                self.started = true;
            }
            Event::Schema { kind, name, sql } if self.started => {
                if kind == "table"
                    && !self.data_started
                    && !self.deferred
                    && !name.starts_with("sqlite_")
                    && self.tables.insert(name.clone())
                {
                    self.tx.execute_batch(&sql)?;
                } else if ["index", "view", "trigger"].contains(&kind.as_str())
                    && self.tables.iter().all(|t| self.copied.contains(t))
                {
                    self.deferred = true;
                    self.tx.execute_batch(&sql)?;
                    if kind == "view" {
                        self.views.push(name);
                    }
                } else {
                    return Err("invalid schema ordering".into());
                }
            }
            Event::Table { name, columns } if self.started && !self.deferred => {
                self.data_started = true;
                if columns.is_empty() || columns.len() > 32767 || !self.copied.insert(name.clone())
                {
                    return Err("invalid or duplicate table".into());
                }
                if name == "sqlite_sequence" {
                    let exists: bool = self.tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='sqlite_sequence')",
                        [],
                        |r| r.get(0),
                    )?;
                    if !exists {
                        let mut init = "__ntdb_sequence_init".to_string();
                        while self.tables.contains(&init) {
                            init.push('_');
                        }
                        self.tx.execute_batch(&format!("CREATE TABLE {} (id INTEGER PRIMARY KEY AUTOINCREMENT); DROP TABLE {};",quote(&init),quote(&init)))?;
                    }
                    self.tx.execute("DELETE FROM sqlite_sequence", [])?;
                } else if !self.tables.contains(&name) {
                    return Err("unknown table".into());
                }
                let sql = format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    quote(&name),
                    columns
                        .iter()
                        .map(|n| quote(n))
                        .collect::<Vec<_>>()
                        .join(","),
                    vec!["?"; columns.len()].join(",")
                );
                // Prepare before accepting records: missing extensions fail even for an empty table.
                self.tx.prepare_cached(&sql)?;
                self.active = Some((sql, columns.len()));
            }
            Event::Done if self.started && self.tables.iter().all(|t| self.copied.contains(t)) => {
                self.done = true
            }
            _ => return Err("invalid stream ordering".into()),
        }
        Ok(())
    }
    pub fn finish(self) -> Result<()> {
        if !self.done || self.active.is_some() {
            return Err("incomplete result stream".into());
        }
        log::info!("阶段：验证 schema 依赖");
        for view in &self.views {
            self.tx
                .prepare(&format!("SELECT * FROM {} LIMIT 0", quote(view)))?;
        }
        for table in &self.tables {
            // EXPLAIN compiles every trigger without running it or changing data.
            self.tx.prepare(&format!(
                "EXPLAIN INSERT INTO {} DEFAULT VALUES",
                quote(table)
            ))?;
            self.tx
                .prepare(&format!("EXPLAIN DELETE FROM {}", quote(table)))?;
            let columns = self
                .tx
                .prepare(&format!("PRAGMA table_xinfo({})", quote(table)))?
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(6)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let assignments = columns
                .iter()
                .filter(|(_, hidden)| *hidden == 0)
                .map(|(name, _)| format!("{0}={0}", quote(name)))
                .collect::<Vec<_>>()
                .join(",");
            if !assignments.is_empty() {
                self.tx.prepare(&format!(
                    "EXPLAIN UPDATE {} SET {}",
                    quote(table),
                    assignments
                ))?;
            }
        }
        log::info!("阶段：重建优化统计");
        self.tx.execute_batch("ANALYZE")?;
        log::info!("阶段：输出数据库完整性检查");
        integrity(&self.tx)?;
        self.tx.commit()?;
        Ok(())
    }
}
/// `finished` must validate EOF and child exit before the target is committed.
pub fn consume(
    conn: &mut Connection,
    mut next: impl FnMut() -> Result<Event>,
    finished: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let mut sink = Consumer::new(conn)?;
    while !sink.done {
        sink.accept(next()?)?;
    }
    finished()?;
    sink.finish()
}
pub fn native_vfs() -> &'static str {
    if cfg!(windows) { "win32" } else { "unix" }
}
/// Owns a newly created final output; removes it unless export completes.
pub struct Output {
    pub path: PathBuf,
    complete: bool,
}
impl Output {
    pub fn create(destination: &Path) -> Result<Self> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        Ok(Self {
            path: destination.to_owned(),
            complete: false,
        })
    }
    pub fn open(&self) -> Result<Connection> {
        Ok(Connection::open_with_flags_and_vfs(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
            native_vfs(),
        )?)
    }
    pub fn finish(mut self) {
        self.complete = true;
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        let _ = fs::remove_file(&self.path);
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut p = self.path.as_os_str().to_owned();
            p.push(suffix);
            let _ = fs::remove_file(PathBuf::from(p));
        }
    }
}
fn require_standard(value: u32) -> Result<()> {
    if value != super::pending::STANDARD {
        return Err(
            "nonstandard pending-byte configuration requires process isolation; use CLI export"
                .into(),
        );
    }
    Ok(())
}
pub fn export_connection(source: &Connection, destination: &Path) -> Result<()> {
    require_standard(super::pending::current())?;
    let output = Output::create(destination)?;
    let mut conn = output.open()?;
    let mut sink = Consumer::new(&mut conn)?;
    produce(source, |event| sink.accept(event))?;
    sink.finish()?;
    drop(conn);
    output.finish();
    Ok(())
}
// Versioned length-prefixed binary wire format, also used for the configuration frame.
pub fn write_frame(w: &mut impl Write, data: &[u8]) -> Result<()> {
    if data.len() > MAX_FRAME {
        return Err("protocol frame too large".into());
    }
    w.write_all(&VERSION.to_le_bytes())?;
    w.write_all(&(data.len() as u32).to_le_bytes())?;
    w.write_all(data)?;
    Ok(())
}
pub fn read_frame(r: &mut impl Read) -> Result<Vec<u8>> {
    let mut v = [0; 2];
    r.read_exact(&mut v)?;
    if u16::from_le_bytes(v) != VERSION {
        return Err("unsupported reader protocol version".into());
    }
    let mut n = [0; 4];
    r.read_exact(&mut n)?;
    let n = u32::from_le_bytes(n) as usize;
    if n > MAX_FRAME {
        return Err("protocol frame too large".into());
    }
    let mut data = Vec::new();
    data.try_reserve_exact(n)?;
    data.resize(n, 0);
    r.read_exact(&mut data)?;
    Ok(data)
}
pub fn put_bytes(b: &mut Vec<u8>, data: &[u8]) {
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(data);
}
pub struct Decoder<'a>(pub &'a [u8]);
impl<'a> Decoder<'a> {
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.0.len() {
            return Err("truncated protocol payload".into());
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    pub fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn integer(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into()?))
    }
    pub fn count(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?) as usize)
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>> {
        let n = self.count()?;
        Ok(self.take(n)?.to_vec())
    }
    pub fn string(&mut self) -> Result<String> {
        Ok(String::from_utf8(self.bytes()?)?)
    }
    pub fn end(self) -> Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err("trailing protocol payload".into())
        }
    }
}
pub fn write_event(w: &mut impl Write, event: &Event) -> Result<()> {
    let mut b = Vec::new();
    match event {
        Event::Metadata(u, a) => {
            b.push(0);
            b.extend_from_slice(&u.to_le_bytes());
            b.extend_from_slice(&a.to_le_bytes());
        }
        Event::Schema { kind, name, sql } => {
            b.push(1);
            for s in [kind, name, sql] {
                put_bytes(&mut b, s.as_bytes());
            }
        }
        Event::Table { name, columns } => {
            b.push(2);
            put_bytes(&mut b, name.as_bytes());
            b.extend_from_slice(&(columns.len() as u32).to_le_bytes());
            for s in columns {
                put_bytes(&mut b, s.as_bytes());
            }
        }
        Event::Row(cells) => {
            b.push(3);
            b.extend_from_slice(&(cells.len() as u32).to_le_bytes());
            for c in cells {
                match c {
                    Cell::Null => b.push(0),
                    Cell::Integer(v) => {
                        b.push(1);
                        b.extend_from_slice(&v.to_le_bytes());
                    }
                    Cell::Real(v) => {
                        b.push(2);
                        b.extend_from_slice(&v.to_le_bytes());
                    }
                    Cell::Text(v) => {
                        b.push(3);
                        put_bytes(&mut b, v);
                    }
                    Cell::Blob(v) => {
                        b.push(4);
                        put_bytes(&mut b, v);
                    }
                }
            }
        }
        Event::End => b.push(4),
        Event::Done => b.push(5),
        Event::Error(e) => {
            b.push(6);
            put_bytes(&mut b, e.as_bytes());
        }
    }
    write_frame(w, &b)
}
pub fn read_event(r: &mut impl Read) -> Result<Event> {
    let b = read_frame(r)?;
    let mut d = Decoder(&b);
    let e = match d.byte()? {
        0 => Event::Metadata(d.integer()?, d.integer()?),
        1 => Event::Schema {
            kind: d.string()?,
            name: d.string()?,
            sql: d.string()?,
        },
        2 => {
            let name = d.string()?;
            let n = d.count()?;
            if n > 32767 {
                return Err("too many columns".into());
            }
            let mut columns = Vec::new();
            for _ in 0..n {
                columns.push(d.string()?);
            }
            Event::Table { name, columns }
        }
        3 => {
            let n = d.count()?;
            if n > 32767 {
                return Err("too many values".into());
            }
            let mut cells = Vec::new();
            for _ in 0..n {
                cells.push(match d.byte()? {
                    0 => Cell::Null,
                    1 => Cell::Integer(d.integer()?),
                    2 => Cell::Real(f64::from_le_bytes(d.take(8)?.try_into()?)),
                    3 => Cell::Text(d.bytes()?),
                    4 => Cell::Blob(d.bytes()?),
                    _ => return Err("unknown value type".into()),
                });
            }
            Event::Row(cells)
        }
        4 => Event::End,
        5 => Event::Done,
        6 => Event::Error(d.string()?),
        _ => return Err("unknown message type".into()),
    };
    d.end()?;
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn memory() -> Connection {
        Connection::open_in_memory_with_flags_and_vfs(
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            native_vfs(),
        )
        .unwrap()
    }
    #[test]
    fn preserves_schema_values_and_rowids() {
        let source = memory();
        source.execute_batch(r#"
            PRAGMA user_version=42;PRAGMA application_id=1234;
            CREATE TABLE "numeric"("40001", "40002", "40003", "40004", "40005", "40006");
            INSERT INTO numeric(rowid,"40001","40002","40003","40004","40005","40006")
              VALUES(99,NULL,-9223372036854775808,1.25,CAST(x'ff0061' AS TEXT),x'0001ff','');
            CREATE TABLE generated(a INTEGER,b INTEGER GENERATED ALWAYS AS(a*2) STORED,c AS(a+1) VIRTUAL);
            INSERT INTO generated(rowid,a) VALUES(71,5);
            CREATE TABLE wr("1" TEXT PRIMARY KEY,"2" BLOB) WITHOUT ROWID;
            INSERT INTO wr VALUES('key',x'00ff');
            CREATE TABLE seq(id INTEGER PRIMARY KEY AUTOINCREMENT,v TEXT);
            INSERT INTO seq VALUES(10,'keep'),(100,'deleted');DELETE FROM seq WHERE id=100;
            CREATE TABLE shadow(rowid TEXT,v TEXT);INSERT INTO shadow(_rowid_,rowid,v) VALUES(17,'shadow','v');
            CREATE TABLE audit(x);
            CREATE INDEX numeric_idx ON numeric("40003");
            CREATE VIEW vv AS SELECT "40003" FROM numeric;
            CREATE TRIGGER trace AFTER INSERT ON generated BEGIN INSERT INTO audit VALUES(new.a);END;
        "#).unwrap();
        let mut target = memory();
        let mut consumer = Consumer::new(&mut target).unwrap();
        produce(&source, |event| {
            let mut wire = Vec::new();
            write_event(&mut wire, &event)?;
            consumer.accept(read_event(&mut &wire[..])?)
        })
        .unwrap();
        consumer.finish().unwrap();
        assert_eq!(
            target
                .query_row("SELECT rowid FROM numeric", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            99
        );
        let mut stmt = target.prepare("SELECT * FROM numeric").unwrap();
        let row = stmt
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .get_ref(3)
            .unwrap()
            .as_bytes()
            .unwrap()
            .to_vec();
        assert_eq!(row, vec![255, 0, 97]);
        assert_eq!(
            target
                .query_row("SELECT rowid+b+c FROM generated", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            87
        );
        assert_eq!(
            target
                .query_row("SELECT _rowid_ FROM shadow", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            17
        );
        assert_eq!(
            target
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='seq'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            100
        );
        assert_eq!(
            target
                .query_row("SELECT count(*) FROM audit", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        target
            .execute("INSERT INTO generated(a) VALUES(8)", [])
            .unwrap();
        assert_eq!(
            target
                .query_row("SELECT x FROM audit", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            8
        );
        for pragma in ["user_version", "application_id"] {
            let a: i64 = source
                .query_row(&format!("PRAGMA {pragma}"), [], |r| r.get(0))
                .unwrap();
            let b: i64 = target
                .query_row(&format!("PRAGMA {pragma}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(a, b);
        }
        let schema = |c: &Connection| {
            c.prepare("SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name").unwrap()
            .query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        assert_eq!(schema(&source), schema(&target));
        assert_eq!(
            target
                .query_row("SELECT hex(\"2\") FROM wr", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "00FF"
        );
    }
    #[test]
    fn validates_wire_and_completion() {
        let event = Event::Row(vec![
            Cell::Null,
            Cell::Integer(i64::MAX),
            Cell::Real(-0.0),
            Cell::Text(vec![255, 0]),
            Cell::Blob(vec![]),
        ]);
        let mut bytes = Vec::new();
        write_event(&mut bytes, &event).unwrap();
        assert_eq!(read_event(&mut &bytes[..]).unwrap(), event);
        for n in 0..bytes.len() {
            assert!(read_event(&mut &bytes[..n]).is_err());
        }
        let mut bad = bytes.clone();
        bad[0] = 99;
        assert!(read_event(&mut &bad[..]).is_err());
        let bad = [1, 0, 255, 255, 255, 255];
        assert!(read_event(&mut &bad[..]).is_err());
        let mut target = memory();
        let events = [
            Event::Metadata(0, 0),
            Event::Schema {
                kind: "table".into(),
                name: "t".into(),
                sql: "CREATE TABLE t(x)".into(),
            },
            Event::Done,
        ];
        let mut events = events.into_iter();
        assert!(consume(&mut target, || Ok(events.next().unwrap()), || Ok(())).is_err());
        assert_eq!(
            target
                .query_row("SELECT count(*) FROM sqlite_schema", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let mut events = [Event::Metadata(0, 0), Event::Done].into_iter();
        assert!(
            consume(
                &mut target,
                || Ok(events.next().unwrap()),
                || Err("child failed".into())
            )
            .is_err()
        );
    }
    #[test]
    fn empty_database_and_orphan_sequence() {
        for sequence in [false, true] {
            let source = memory();
            if sequence {
                source
                    .execute_batch(
                        "CREATE TABLE old(id INTEGER PRIMARY KEY AUTOINCREMENT);DROP TABLE old",
                    )
                    .unwrap();
            }
            let mut target = memory();
            let mut sink = Consumer::new(&mut target).unwrap();
            produce(&source, |e| sink.accept(e)).unwrap();
            sink.finish().unwrap();
            if sequence {
                assert_eq!(
                    target
                        .query_row("SELECT count(*) FROM sqlite_sequence", [], |r| r
                            .get::<_, i64>(0))
                        .unwrap(),
                    0
                );
            }
        }
    }
    #[test]
    fn virtual_table_fails_explicitly() {
        let source = memory();
        source
            .execute_batch("CREATE VIRTUAL TABLE t USING fts5(x);INSERT INTO t VALUES('data')")
            .unwrap();
        assert!(
            produce(&source, |_| Ok(()))
                .unwrap_err()
                .to_string()
                .contains("virtual table")
        );
        assert!(source.is_autocommit());
    }
    #[test]
    fn write_failure_rolls_back() {
        let mut target = memory();
        let mut sink = Consumer::new(&mut target).unwrap();
        sink.accept(Event::Metadata(0, 0)).unwrap();
        sink.accept(Event::Schema {
            kind: "table".into(),
            name: "t".into(),
            sql: "CREATE TABLE t(x INTEGER NOT NULL)".into(),
        })
        .unwrap();
        sink.accept(Event::Table {
            name: "t".into(),
            columns: vec!["x".into()],
        })
        .unwrap();
        assert!(sink.accept(Event::Row(vec![Cell::Null])).is_err());
        drop(sink);
        assert_eq!(
            target
                .query_row("SELECT count(*) FROM sqlite_schema", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn existing_output_is_never_changed() {
        let root = std::env::current_dir().unwrap().join(".tmp");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!("existing-output-test-{}.db", std::process::id()));
        fs::write(&path, b"untouched").unwrap();
        assert!(export_connection(&memory(), &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"untouched");
        fs::remove_file(&path).unwrap();
        let output = Output::create(&path).unwrap();
        assert_eq!(output.path, path);
        assert!(path.exists());
        drop(output);
        assert!(!path.exists());
        let source = memory();
        source
            .execute_batch("CREATE VIRTUAL TABLE t USING fts5(x)")
            .unwrap();
        assert!(export_connection(&source, &path).is_err());
        assert!(!path.exists());
    }
    #[test]
    fn missing_view_and_trigger_extensions_fail() {
        for schema in [
            "CREATE VIEW v AS SELECT missing_extension(x) FROM t",
            "CREATE TRIGGER tr AFTER INSERT ON t BEGIN SELECT missing_extension(new.x);END",
        ] {
            let source = memory();
            source.execute_batch("CREATE TABLE t(x)").unwrap();
            source.execute_batch(schema).unwrap();
            let mut target = memory();
            let mut sink = Consumer::new(&mut target).unwrap();
            produce(&source, |e| sink.accept(e)).unwrap();
            assert!(
                sink.finish()
                    .unwrap_err()
                    .to_string()
                    .contains("missing_extension")
            );
            assert_eq!(
                target
                    .query_row("SELECT count(*) FROM sqlite_schema", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        assert!(
            require_standard(super::super::pending::STANDARD - 1024)
                .unwrap_err()
                .to_string()
                .contains("process isolation")
        );
    }
    #[test]
    fn snapshot_and_direct_library_export() {
        let root = std::env::current_dir().unwrap().join(".tmp");
        fs::create_dir_all(&root).unwrap();
        let dir = root.join(format!("library-snapshot-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let source_path = dir.join("source.db");
        let source = Connection::open_with_flags_and_vfs(
            &source_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            native_vfs(),
        )
        .unwrap();
        source
            .execute_batch(
                "PRAGMA journal_mode=WAL;CREATE TABLE t(x);INSERT INTO t VALUES(1),(2),(3)",
            )
            .unwrap();
        let writer = Connection::open_with_flags_and_vfs(
            &source_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
            native_vfs(),
        )
        .unwrap();
        let mut target = memory();
        let mut sink = Consumer::new(&mut target).unwrap();
        let mut changed = false;
        produce(&source, |event| {
            if matches!(&event, Event::Row(_)) && !changed {
                writer.execute("UPDATE t SET x=x+100", [])?;
                changed = true;
            }
            sink.accept(event)
        })
        .unwrap();
        sink.finish().unwrap();
        assert_eq!(
            target
                .query_row("SELECT sum(x) FROM t", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            6
        );
        let out = dir.join("library's output.db");
        super::super::export_to_plain(&source, &out).unwrap();
        let exported = Connection::open_with_flags_and_vfs(
            &out,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
            native_vfs(),
        )
        .unwrap();
        assert_eq!(
            exported
                .query_row("SELECT sum(x) FROM t", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            306
        );
        integrity(&exported).unwrap();
        drop(exported);
        drop(writer);
        drop(source);
        assert_eq!(
            fs::canonicalize(&dir).unwrap().parent(),
            Some(fs::canonicalize(root).unwrap().as_path())
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
