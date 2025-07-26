use anyhow::anyhow;
use chrono::TimeZone;
use eventix_ical::{
    col::AlarmOccurrence,
    objects::{AlarmOverlay, CalAlarm, CalComponent, CalDate, DefaultAlarmOverlay, EventLike},
};
use eventix_state::{CalendarAlarmType, EventixState, PersonalCalendarAlarms};
use std::{
    collections::HashMap,
    path::{self, PathBuf},
    process::Command,
    sync::Arc,
};
use tokio::time;
use tracing::{info, warn};

use crate::locale::{DateFlags, Locale};

struct Notification {
    pub appname: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
}

impl Notification {
    fn from_alarm(occ: &AlarmOccurrence<'_>, locale: &Arc<dyn Locale + Send + Sync>) -> Self {
        let mut body = String::new();
        if let Some(start) = occ.occurrence().occurrence_start() {
            body = format!("Start: {}", locale.fmt_datetime(&start, DateFlags::None));
        }
        if let Some(loc) = occ.occurrence().location() {
            body.push_str(&format!("\nWhere: {loc}"));
        }
        body.push_str(&format!(
            "\nReminder for: {}",
            locale.fmt_datetime(&occ.alarm_date().unwrap(), DateFlags::Short)
        ));

        let icon: PathBuf = ["static", "images", "icon.png"].iter().collect();
        let icon = path::absolute(icon).unwrap();
        Self {
            appname: String::from("Eventix"),
            icon: icon.into_os_string().into_string().unwrap(),
            summary: occ
                .occurrence()
                .summary()
                .cloned()
                .unwrap_or(String::from("?")),
            body,
        }
    }

    fn send(&self) -> anyhow::Result<()> {
        let mut args = vec![];
        args.push(format!("--app-name={}", self.appname));
        args.push(format!("--icon={}", self.icon));
        args.push("--urgency=critical".to_string());
        args.push(self.summary.clone());
        args.push(self.body.clone());
        Command::new("/usr/bin/notify-send")
            .args(args)
            .spawn()
            .map_err(|e| anyhow!(e).context("running notify-send"))?;
        Ok(())
    }
}

struct NotifyAlarmOverlay<'a> {
    personal: Option<&'a PersonalCalendarAlarms>,
    default: &'a Option<CalAlarm>,
}

impl<'a> NotifyAlarmOverlay<'a> {
    fn new(personal: Option<&'a PersonalCalendarAlarms>, default: &'a Option<CalAlarm>) -> Self {
        Self { personal, default }
    }
}

impl AlarmOverlay for NotifyAlarmOverlay<'_> {
    fn alarms_for_component(&self, comp: &CalComponent) -> Option<Vec<CalAlarm>> {
        if let Some(personal) = self.personal {
            match personal.get(comp.uid(), None) {
                Some(overwrite) => Some(overwrite.alarms().to_vec()),
                None => self.default.clone().map(|alarm| vec![alarm]),
            }
        } else {
            self.default.clone().map(|alarm| vec![alarm])
        }
    }

    fn alarm_overwrites(
        &self,
        comp: &CalComponent,
        overwrites: HashMap<CalDate, &[CalAlarm]>,
    ) -> HashMap<CalDate, Vec<CalAlarm>> {
        // we only need to specify our personal overwrites here as we already specify the default
        // for the base component above. So, we automatically fall back to this alarm if we have no
        // overwrite for a specific occurrence.
        if let Some(personal) = self.personal {
            let mut personal = personal.all_for_occurrences(comp.uid());
            for (rid, alarms) in overwrites {
                personal.entry(rid).or_insert_with(|| alarms.to_vec());
            }
            personal
        } else {
            HashMap::default()
        }
    }
}

pub async fn watch_alarms(state: EventixState, locale: Arc<dyn Locale + Send + Sync>) {
    loop {
        {
            let mut state = state.lock().await;
            let last_check = chrono::Utc
                .from_utc_datetime(&state.misc().last_alarm_check())
                .with_timezone(locale.timezone());
            let now = chrono::Utc::now().with_timezone(locale.timezone());

            // find all due alarms since the last check and sort them by alarm time
            let mut alarms = state
                .store()
                .directories()
                .iter()
                .flat_map(|dir| {
                    let overlay: Box<dyn AlarmOverlay> =
                        match state.settings().calendar(dir.id()).unwrap().alarms() {
                            CalendarAlarmType::Personal { default } => {
                                Box::new(NotifyAlarmOverlay::new(
                                    state.personal_alarms().get(dir.id()),
                                    default,
                                ))
                            }
                            CalendarAlarmType::Calendar => Box::new(DefaultAlarmOverlay),
                        };
                    dir.due_alarms_between(last_check, now, &*overlay)
                        .collect::<Vec<_>>()
                })
                .filter(|a| !a.occurrence().is_cancelled())
                .collect::<Vec<_>>();
            alarms.sort_by_key(|o| o.alarm_date().unwrap());

            for alarm in alarms {
                let notification = Notification::from_alarm(&alarm, &locale);
                info!(
                    "Sending alarm notification for {} (start={:?}, due={:?})",
                    alarm.occurrence().uid(),
                    alarm.occurrence().occurrence_start(),
                    alarm.alarm_date()
                );
                notification.send().unwrap();
            }

            let misc = state.misc_mut();
            misc.set_last_alarm_check(now.naive_utc());
            // permanently remember last time of check
            if let Err(e) = misc.write_to_file() {
                warn!("Unable to save misc state: {}", e);
            }
        }

        time::sleep(time::Duration::from_secs(30)).await;
    }
}
