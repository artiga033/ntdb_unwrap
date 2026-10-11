use crate::{Error, Result};
use ntdb_unwrap::db::stream;
use std::path::PathBuf;
pub struct Export {
    prepared: super::common::Prepared,
    output_file: PathBuf,
}
pub fn export(matches: clap::ArgMatches) -> Result<Export> {
    let output_file = matches.get_one::<PathBuf>("output").unwrap().clone();
    if output_file.exists() {
        return Err(Error::App {
            message: "输出文件已存在，拒绝覆盖".into(),
        });
    }
    let prepared = super::common::prepare(&matches)?;
    Ok(Export {
        prepared,
        output_file,
    })
}
impl super::App for Export {
    fn run(self: Box<Self>) -> Result<()> {
        let config = super::reader::Config::from_prepared(&self.prepared);
        let result = (|| -> stream::Result<()> {
            let output = stream::Output::create(&self.output_file)?;
            let mut target = output.open()?;
            let mut reader = super::reader::Reader::spawn(&config)?;
            reader.consume(&mut target)?;
            drop(reader);
            drop(target);
            output.finish();
            Ok(())
        })();
        if let Err(e) = result {
            let message = e.to_string();
            if let Some(hint) = super::reader::diagnose(&config, &message) {
                eprintln!("{hint}");
            }
            return Err(Error::App { message });
        }
        println!("已导出为未加密数据库：{:?}", self.output_file);
        Ok(())
    }
}
