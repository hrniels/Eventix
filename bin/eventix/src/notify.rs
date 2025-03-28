use anyhow::anyhow;
use chrono::TimeZone;
use ical::{
    col::AlarmOccurrence,
    objects::{AlarmOverlay, CalAlarm, CalComponent, CalDate, DefaultAlarmOverlay, EventLike},
};
use std::{
    collections::HashMap,
    path::{self, PathBuf},
    process::Command,
    sync::Arc,
};
use tokio::time;
use tracing::warn;

use crate::{
    locale::{DateFlags, Locale},
    state::EventixState,
    state::PersonalCalendarAlarms,
};

struct Notification {
    pub appname: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub timeout: i32,
}

impl Notification {
    fn from_alarm(occ: &AlarmOccurrence<'_>, locale: &Arc<dyn Locale + Send + Sync>) -> Self {
        let mut body = String::new();
        if let Some(start) = occ.occurrence().occurrence_start() {
            body = format!("Start: {}", locale.fmt_datetime(&start, DateFlags::None));
        }
        if let Some(loc) = occ.occurrence().location() {
            body.push_str(&format!("\nWhere: {}", loc));
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
            timeout: 24 * 3600 * 1000,
        }
    }

    fn send(&self) -> anyhow::Result<()> {
        let mut args = vec![];
        args.push(format!("--app-name={}", self.appname));
        args.push(format!("--icon={}", self.icon));
        args.push(format!("--expire-time={}", self.timeout));
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
    default: DefaultAlarmOverlay,
}

impl<'a> NotifyAlarmOverlay<'a> {
    fn new(personal: Option<&'a PersonalCalendarAlarms>) -> Self {
        Self {
            personal,
            default: DefaultAlarmOverlay,
        }
    }
}

impl AlarmOverlay for NotifyAlarmOverlay<'_> {
    fn alarms_for_component(&self, comp: &CalComponent) -> Option<Vec<CalAlarm>> {
        if let Some(personal) = self.personal {
            personal.get(comp.uid(), None).map(|a| a.alarms().to_vec())
        } else {
            self.default.alarms_for_component(comp)
        }
    }

    fn alarm_overwrites(
        &self,
        comp: &CalComponent,
        overwrites: HashMap<CalDate, &[CalAlarm]>,
    ) -> HashMap<CalDate, Vec<CalAlarm>> {
        if let Some(personal) = self.personal {
            let mut personal = personal.all_for_occurrences(comp.uid());
            for (rid, alarms) in overwrites {
                personal.entry(rid).or_insert_with(|| alarms.to_vec());
            }
            personal
        } else {
            self.default.alarm_overwrites(comp, overwrites)
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
                    let overlay = NotifyAlarmOverlay::new(state.personal_alarms().get(dir.id()));
                    dir.due_alarms_between(last_check, now, &overlay)
                        .collect::<Vec<_>>()
                })
                .filter(|a| !a.occurrence().is_cancelled())
                .collect::<Vec<_>>();
            alarms.sort_by_key(|o| o.alarm_date().unwrap());

            for alarm in alarms {
                let notification = Notification::from_alarm(&alarm, &locale);
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
