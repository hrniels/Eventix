use std::{io, path::Path};

use crate::{LocaleType, Translations};

use super::Locale;

#[derive(Default, Debug)]
pub struct LocaleEn {
    trans: Translations,
}

impl LocaleEn {
    pub fn new(path: &Path) -> io::Result<Self> {
        let trans = Translations::new_from_file(path)?;
        Ok(Self { trans })
    }
}

impl Locale for LocaleEn {
    fn ty(&self) -> LocaleType {
        LocaleType::English
    }

    fn translations(&self) -> &Translations {
        &self.trans
    }
}
