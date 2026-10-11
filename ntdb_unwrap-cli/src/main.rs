mod app;
use std::path::PathBuf;

use clap::{Arg, ArgAction, Command, arg, command, value_parser};
use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(context(false), display("{source}"))]
    Underlying { source: ntdb_unwrap::Error },
    #[snafu(context(false))]
    IO { source: std::io::Error },
    #[snafu(display("{op}: {source}"))]
    Sqlite { source: rusqlite::Error, op: String },
    #[snafu(whatever, display("{message}"))]
    App { message: String },
}
pub type Result<T> = std::result::Result<T, Error>;
fn main() -> Result<()> {
    env_logger::init();
    let mut matches = cmd().get_matches();
    if matches.subcommand_name() == Some("__internal-db-reader") {
        if app::reader::internal().is_err() {
            std::process::exit(1);
        }
        return Ok(());
    }
    #[cfg(windows)]
    let _cancellation = if matches.subcommand_name() != Some("serve") {
        Some(
            ntdb_unwrap::db::job::cancellation().map_err(|e| Error::App {
                message: e.to_string(),
            })?,
        )
    } else {
        None
    };
    let app: Box<dyn app::App> = match matches.remove_subcommand() {
        Some((s, matches)) if s == "export" => Box::new(app::export(matches)?),
        #[cfg(feature = "serve")]
        Some((s, matches)) if s == "serve" => Box::new(app::serve(matches)?),
        #[cfg(not(feature = "serve"))]
        Some((s, _)) if s == "serve" => {
            return Err(Error::App {
                message: "serve feature is not enabled".into(),
            });
        }
        _ => Box::new(app::export(subcommand_export().get_matches())?),
    };
    app.run()?;
    Ok(())
}

fn common_args() -> [Arg; 5] {
    [
        arg!([file] "NT QQ 数据库文件。如果未提供，将尝试自动检测"),
        arg!(-p --pkey <pkey> "数据库密钥。如果未提供，将尝试自动探测"),
        arg!(-N --nocopy "默认情况下，会先将db文件复制到一个临时文件，再去操作临时文件。启用此选项以直接读取原始数据库文件。注意：这可能损坏你的数据库！")
        .action(ArgAction::SetTrue),
        arg!(--"android-uid" <UID> "如果确信这是一个 android NTQQ 的数据库，那么提供 uid 可以直接解密"),
        Arg::new("pending-byte-offset").long("pending-byte-offset").value_name("BYTES")
            .value_parser(value_parser!(i64)).allow_negative_numbers(true)
            .help("保留页偏移（相对 0x40000000）；默认不修改。例如 --pending-byte-offset=-1024")
    ]
}
fn subcommand_export() -> Command {
    command!("export")
        .about("导出为未加密 sqlite 数据库")
        .args(common_args())
        .args([arg!(-o --output <PATH> "输出文件")
            .value_parser(value_parser!(PathBuf))
            .default_value("./nt_unwraped.db")])
}
fn cmd() -> Command {
    command!()
        .about("一键解密/解析 NTQQ 数据库！")
        .after_help(
            "可以不带任何subcommand运行此程序，默认进入 export 模式，并尝试自动探测所有参数。",
        )
        .args_conflicts_with_subcommands(true)
        .subcommand(Command::new("__internal-db-reader").hide(true))
        .subcommand(subcommand_export())
        .subcommand(
            command!("serve")
                .about("启动一个 web 服务，以通过 HTTP API 读取数据库内容。")
                .args(common_args())
                .args([arg!(-l --listen [listen] "监听地址")
                    .value_parser(value_parser!(std::net::SocketAddr))
                    .default_value("127.0.0.1:19551")]),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_byte_arguments() {
        for args in [
            vec!["export", "--pending-byte-offset", "-1024"],
            vec!["export", "--pending-byte-offset=-1024"],
            vec!["serve", "--pending-byte-offset", "-1024"],
        ] {
            let m = cmd()
                .try_get_matches_from(std::iter::once("ntdb_unwrap").chain(args))
                .unwrap();
            assert_eq!(
                m.subcommand()
                    .unwrap()
                    .1
                    .get_one::<i64>("pending-byte-offset"),
                Some(&-1024)
            );
        }
        for value in ["0", "1024", "-1073741824", "4294967295"] {
            let m = cmd()
                .try_get_matches_from(["ntdb_unwrap", "export", "--pending-byte-offset", value])
                .unwrap();
            assert_eq!(
                m.subcommand()
                    .unwrap()
                    .1
                    .get_one::<i64>("pending-byte-offset")
                    .unwrap()
                    .to_string(),
                value
            );
        }
        for value in ["invalid", "9223372036854775808"] {
            assert!(
                cmd()
                    .try_get_matches_from(["ntdb_unwrap", "export", "--pending-byte-offset", value])
                    .is_err()
            );
        }
        assert!(
            !cmd()
                .render_help()
                .to_string()
                .contains("__internal-db-reader")
        );
        let m = cmd()
            .try_get_matches_from(["ntdb_unwrap", "__internal-db-reader"])
            .unwrap();
        assert_eq!(m.subcommand_name(), Some("__internal-db-reader"));
        let m = cmd()
            .try_get_matches_from(["ntdb_unwrap", "export"])
            .unwrap();
        assert!(
            m.subcommand()
                .unwrap()
                .1
                .get_one::<i64>("pending-byte-offset")
                .is_none()
        );
    }
}
