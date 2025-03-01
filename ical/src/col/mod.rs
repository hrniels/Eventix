use std::{io, path::PathBuf};

use crate::parser::ParseError;
use thiserror::Error;

mod file;
mod occurrence;
mod source;
mod store;

pub use file::CalFile;
pub use occurrence::Occurrence;
pub use source::CalSource;
pub use store::CalStore;

#[derive(Debug, Error)]
pub enum ColError {
    #[error("Reading directory {0} failed: {1}")]
    ReadDir(PathBuf, io::Error),
    #[error("Opening {0} failed: {1}")]
    FileOpen(PathBuf, io::Error),
    #[error("Reading {0} failed: {1}")]
    FileRead(PathBuf, io::Error),
    #[error("Writing {0} failed: {1}")]
    FileWrite(PathBuf, io::Error),
    #[error("Removing {0} failed: {1}")]
    FileRemove(PathBuf, io::Error),
    #[error("Parsing {0} failed: {1}")]
    FileParse(PathBuf, ParseError),
    #[error("Getting file type for {0} failed: {1}")]
    FileType(PathBuf, io::Error),
    #[error("Unable to find source with id {0}")]
    SourceNotFound(String),
    #[error("Unable to find file with path {0}")]
    FileNotFound(PathBuf),
}
