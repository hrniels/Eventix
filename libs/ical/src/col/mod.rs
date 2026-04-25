// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Collections for iCalendar objects.
//!
//! These collections are not part of RFC 5545, but are provided on top of these to provide a layer
//! of abstraction for applications that want to work with these objects.
//!
//! The first layer on top is provided by [`CalFile`], which represents a file on disk that
//! contains exactly one [`Calendar`](crate::objects::Calendar) object. Besides access to this
//! calendar, it knows the [`CalDir`] it is contained in. So, a [`CalDir`] is a container for
//! several [`CalFile`] objects that all belong together and live in the same directory on disk. On
//! top of that, [`CalStore`] is a container for multiple [`CalDir`] objects.
//!
//! Besides these containers, this module also provides [`Occurrence`], which is used to represent
//! individual occurrences of events/TODOs.

use std::{io, path::PathBuf};

use crate::{objects::CalDate, parser::ParseError};
use thiserror::Error;

mod dir;
mod file;
mod occurrence;
mod store;

pub use dir::CalDir;
pub use file::CalFile;
pub use occurrence::{AlarmOccurrence, EventTzRange, Occurrence};
pub use store::{CalStore, DirectoryWriteGuard};

/// Errors that can occur in the collections module.
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
    #[error("Unable to find directory with id {0}")]
    DirNotFound(String),
    #[error("Directory {0} is write-protected")]
    DirWriteProtected(String),
    #[error("Unable to find file with path {0}")]
    FileNotFound(PathBuf),
    #[error("Component with uid {0} not found")]
    ComponentNotFound(String),
    #[error("An overwrite with rid {0} exists")]
    RidExists(CalDate),
    #[error("Getting metadata of {0} failed")]
    FileMetadata(PathBuf),
    #[error("Getting last modified timestamp of {0} failed")]
    FileModified(PathBuf),
    #[error("Validation failed: {0}")]
    Validation(#[from] ParseError),
}
