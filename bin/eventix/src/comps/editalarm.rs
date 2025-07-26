use anyhow::Context;
use askama::Template;
use eventix_ical::objects::{CalAlarm, CalDate, EventLike};
use std::sync::Arc;
use tokio::sync::MutexGuard;

use crate::comps::alarmconfig::{AlarmConfig, AlarmConfigTemplate};
use crate::html::filters;
use crate::locale::Locale;
use crate::objects::DayOccurrence;
use crate::state::CalendarAlarmType;

#[derive(Template)]
#[template(path = "comps/editalarm.htm")]
pub struct EditAlarmTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    config: AlarmConfigTemplate,
    uid: String,
    rid: Option<String>,
    rid_str: String,
    edit: bool,
    overwrite: bool,
    effective: Option<Vec<CalAlarm>>,
    personal_alarms: bool,
    occ: DayOccurrence<'a>,
}

impl<'a> EditAlarmTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        state: &'a MutexGuard<'a, crate::state::State>,
        uid: String,
        rid: Option<String>,
        edit: bool,
    ) -> anyhow::Result<Self> {
        let rid_date = if let Some(rid) = &rid {
            Some(
                rid.parse::<CalDate>()
                    .context(format!("Invalid rid date: {rid}"))?,
            )
        } else {
            None
        };

        let occ = state
            .store()
            .occurrence_by_id(&uid, rid_date.as_ref(), locale.timezone())
            .context(format!(
                "Unable to find occurrence with uid '{}' and rid '{:?}'",
                &uid, rid_date
            ))?;

        let alarm_type = state.settings().calendar(occ.directory()).unwrap().alarms();
        let personal = if let Some(pers_cal) = state.personal_alarms().get(occ.directory()) {
            pers_cal.get(&uid, rid_date.as_ref())
        } else {
            None
        };

        let effective = state.personal_alarms().effective_alarms(&occ, alarm_type);
        let day_occ = DayOccurrence::new(&occ, None, false, effective.is_some());

        let config = Some(AlarmConfig::from_alarms(
            match personal {
                Some(a) => a.alarms(),
                None => &[],
            },
            locale.timezone(),
        ));

        Ok(Self {
            edit,
            effective,
            uid,
            rid: rid.clone(),
            rid_str: rid.unwrap_or_default(),
            overwrite: personal.is_some(),
            config: AlarmConfigTemplate::new(locale.clone(), String::from("personal"), config),
            personal_alarms: matches!(alarm_type, CalendarAlarmType::Personal { .. }),
            locale,
            occ: day_occ,
        })
    }
}
