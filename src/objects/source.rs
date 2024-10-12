use anyhow::anyhow;
use std::{
    fs::{read_dir, File},
    io::Read,
    path::PathBuf,
};

use super::CalItem;

pub struct CalSource {
    path: PathBuf,
    items: Vec<CalItem>,
}

impl CalSource {
    pub fn new_from_dir(path: PathBuf) -> Result<Self, anyhow::Error> {
        let mut items = Vec::new();
        for e in read_dir(path.as_path())? {
            let filename = e?.path();

            let mut input = String::new();
            File::open(filename.as_path())?.read_to_string(&mut input)?;

            let cal = input.parse::<icalendar::Calendar>().map_err(|e| {
                anyhow!("Parsing calendar in {:?} failed: {}", filename.as_path(), e)
            })?;
            let cal = CalItem::new(filename, cal);
            items.push(cal);
        }
        Ok(Self { path, items })
    }

    pub fn items(&self) -> &[CalItem] {
        &self.items
    }
}
