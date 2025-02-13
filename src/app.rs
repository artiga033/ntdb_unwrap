use crate::{Error, Result, *};
use clap::{arg, command, value_parser, ArgAction, Command};
use ntdb_unwrap::*;
use ntqq::{DBDecryptInfo, UserDBFile};
use rusqlite::OpenFlags;
use snafu::{prelude::*, FromString};
use std::{env, fs, io::Read, path::PathBuf};

pub struct App {
    working_on_temp_file: bool,
    user_db_file: UserDBFile,
    output_file: PathBuf,
    conn: rusqlite::Connection,
}
impl App {
    pub fn init() -> Result<Self> {
        let matches = App::cmd().get_matches();

        let output_file = matches.get_one::<PathBuf>("output").unwrap().to_owned();

        let mut file: UserDBFile = match matches.get_one::<String>("file") {
            Some(f) => UserDBFile {
                path: fs::canonicalize(f)?,
                ..Default::default()
            },
            None => {
                let db_files = ntqq::detect_db_file()?;
                if db_files.len() == 1 {
                    db_files.into_iter().next().unwrap()
                } else {
                    for (i, db_file) in db_files.iter().enumerate() {
                        println!("please choose one of the following db files:");
                        println!("{}. {}", i, db_file);
                    }
                    let mut input = String::new();
                    loop {
                        input.clear();
                        std::io::stdin().read_line(&mut input).unwrap();
                        if let Ok(i) = input.trim().parse::<usize>() {
                            if i < db_files.len() {
                                break db_files.into_iter().nth(i).unwrap();
                            }
                        }
                        println!("Invalid input, please try again");
                    }
                }
            }
        };

        let decrypt_info: DBDecryptInfo = match matches.get_one::<String>("pkey") {
            Some(pkey) => DBDecryptInfo {
                key: pkey.to_owned(),
                ..Default::default()
            },
            None => {
                if let ntqq::Platform::Android = ntqq::get_platform() {
                    if let Some(uid) = &file.uid {
                        let mut f = fs::File::open(&file.path)?;
                        let mut buf = [0u8; 1024];
                        f.read_exact(&mut buf)?;
                        ntqq::android::decode_db_header(uid, &buf).whatever_context::<_, Error>(
                            "failed to decode android nt_qq db header",
                        )?
                    } else {
                        whatever!("uid must be provided for android NT QQ");
                    }
                } else {
                    whatever!("pkey must be provided unless it's android NT QQ");
                }
            }
        };

        let mut working_on_temp_file = false;
        if !matches.get_flag("nocopy") {
            let temp_file = env::temp_dir().join("nt_msg_temp_copy.db");
            println!("copying db file to temp file: {:?}", temp_file);
            fs::copy(&file.path, &temp_file)?;
            working_on_temp_file = true;
            file.path = temp_file;
        } else {
            println!("[WARN] Working on the db file directly. This may break you data!");
        }

        db::register_offset_vfs().map_err(|e| {
            Error::without_source(format!("failed to register offset vfs: sqlite code {}", e))
        })?;
        let conn = rusqlite::Connection::open_with_flags_and_vfs(
            &file.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            db::OFFSET_VFS_NAME,
        )
        .context(SqliteSnafu { op: "open db" })?;

        db::try_decrypt_db(&conn, decrypt_info)?;

        Ok(App {
            user_db_file: file,
            working_on_temp_file,
            conn,
            output_file,
        })
    }

    pub fn run(self) -> Result<()> {
        db::export_to_plain(&self.conn, &self.output_file)?;
        println!("plaintext exported to {:?}", self.output_file);
        Ok(())
    }

    fn cmd() -> Command {
        command!().args([
            arg!([file] "NT QQ DB file. Will try auto detect if not provided"),
            arg!(-p --pkey [pkey] "The pkey of the db. Must br provided unless it's android NT QQ"),
            arg!(-o --output [output] "Output file").value_parser(value_parser!(PathBuf)).default_value("./nt_unwraped.db"),
            arg!(-N --nocopy "By default, will try copy the db file to a a temp file first and work on it. This flag will disable this behavior and work on the file directly. NOTE: This may break the db file!")
            .action(ArgAction::SetTrue),
        ])
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if self.working_on_temp_file {
            println!("clean up temp file: {:?}", self.user_db_file.path);
            fs::remove_file(&self.user_db_file.path).unwrap();
        }
    }
}
