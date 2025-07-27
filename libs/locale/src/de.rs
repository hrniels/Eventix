use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::Locale;

#[derive(Default)]
pub struct LocaleDe {}

impl Locale for LocaleDe {
    fn translate<'a>(&self, key: &'a str) -> &'a str {
        static XLATE_TABLE: Lazy<HashMap<&str, &str>> = Lazy::new(|| {
            HashMap::from([
                ("Events", "Ereignisse"),
                ("Error", "Fehler"),
                ("Errors", "Fehler"),
                ("Information", "Information"),
                ("Yes", "Ja"),
                ("No", "Nein"),
                ("Page generation time", "Ladezeit"),
                // Status names
                ("NEEDS-ACTION", "Needs action"),
                ("COMPLETED", "Completed"),
                ("IN-PROCESS", "In process"),
                ("CANCELLED", "Canceled"),
                ("TENTATIVE", "Tentative"),
                ("CONFIRMED", "Confirmed"),
            ])
        });
        XLATE_TABLE.get(key).unwrap_or(&key)
    }
}
