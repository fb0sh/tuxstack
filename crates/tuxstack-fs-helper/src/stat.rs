//! `stat` command: lstat of a single path (never follows the final symlink).

use crate::error::{HelperError, Result};
use crate::metadata::stat_message;
use crate::path::{self, resolve_token};
use crate::emit;

pub fn run(args: &[String]) -> Result<()> {
    let flags = path::parse_flags(args)?;
    let path = resolve_token(&flags.root, &flags.token)?;
    let meta = std::fs::symlink_metadata(&path).map_err(|error| HelperError::from_io(&path, &error))?;
    let message = stat_message(&path, &flags.token, &meta)?;
    emit(&message);
    Ok(())
}
