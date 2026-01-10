mod nt_msg;
pub use nt_msg::*;
use snafu::ResultExt;

use super::*;
pub trait Model
where
    Self: Sized,
{
    /// Parse a row from a query result into the model.
    /// This expect the row is queried AS IS.
    /// That is, you use `SELECT *`
    fn parse_row(row: &rusqlite::Row) -> crate::Result<Self>;
    fn parse_rows(rows: &mut rusqlite::Rows) -> crate::Result<Vec<Self>> {
        let mut result = Vec::new();
        while let Some(row) = rows.next().context(SqliteSnafu {
            op: "iterating rows next",
        })? {
            result.push(Self::parse_row(row)?);
        }
        Ok(result)
    }
}

macro_rules! map_field {
    ($row:ident,$column:literal) => {
        $row.get($column)
            .context(crate::db::SqliteSnafu {
                op: concat!("parsing column: ", $column),
            })
            .map_err(crate::Error::from)
    };
}

use map_field;
