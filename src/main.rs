mod app;
use app::App;

use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(context(false))]
    Underlying { source: ntdb_unwrap::Error },
    #[snafu(context(false))]
    IO { source: std::io::Error },
    #[snafu()]
    Sqlite { source: rusqlite::Error, op: String },
    #[snafu(whatever, display("{message}"))]
    App { message: String },
}
pub type Result<T> = std::result::Result<T, Error>;
fn main() -> Result<()> {
    let app = App::init()?;
    app.run()?;

    Ok(())
}
