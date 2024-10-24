use super::Locale;

#[derive(Default)]
pub struct LocaleEn {}

impl Locale for LocaleEn {
    fn translate<'a>(&self, key: &'a str) -> &'a str {
        key
    }
}
