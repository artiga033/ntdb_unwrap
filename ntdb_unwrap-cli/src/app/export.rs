use crate::Result;
use ntdb_unwrap::*;
use std::path::PathBuf;

pub struct Export {
    bootstrap: super::common::Bootstrap,
    output_file: PathBuf,
}
pub fn export(matches: clap::ArgMatches) -> Result<Export> {
    let bootstrap = super::common::bootstrap(&matches)?;
    let output_file = matches.get_one::<PathBuf>("output").unwrap().to_owned();
    Ok(Export {
        bootstrap,
        output_file,
    })
}

impl super::App for Export {
    fn run(self: Box<Self>) -> Result<()> {
        db::export_to_plain(&self.bootstrap.conn, &self.output_file)?;
        println!("已导出为未加密数据库：{:?}", self.output_file);
        Ok(())
    }
}
