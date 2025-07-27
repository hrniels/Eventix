use askama::Template;
use eventix_ical::objects::CalPartStat;
use eventix_locale::Locale;
use std::sync::Arc;

use crate::html::filters;

#[derive(Template)]
#[template(path = "comps/partstat.htm")]
pub struct PartStatTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    id: String,
    stat: CalPartStat,
    uid: String,
    rid: Option<String>,
    prefix: String,
    recurrent: bool,
}

impl PartStatTemplate {
    pub fn new<I: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        id: I,
        stat: CalPartStat,
        uid: String,
        rid: Option<String>,
        recurrent: bool,
    ) -> Self {
        let prefix = if recurrent && rid.is_some() {
            locale.translate("(Occ.)")
        } else if recurrent && rid.is_none() {
            locale.translate("(Ser.)")
        } else {
            ""
        }
        .to_string();
        Self {
            locale,
            id: id.to_string(),
            stat,
            uid,
            rid,
            prefix,
            recurrent,
        }
    }
}
