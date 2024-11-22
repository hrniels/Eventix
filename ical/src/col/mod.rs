use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::{io, path::PathBuf};

use crate::parser::ParseError;
use thiserror::Error;

mod item;
mod occurrence;
mod source;
mod store;

pub use item::CalItem;
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
}

pub type Id = u64;

pub fn generate_id() -> Id {
    static NEXT_ID: Lazy<Mutex<Id>> = Lazy::new(|| Mutex::new(0));
    let mut next = NEXT_ID.lock().unwrap();
    let res = *next + 1;
    *next += 1;
    res
}
