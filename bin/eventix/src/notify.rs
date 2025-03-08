use anyhow::anyhow;
use chrono::TimeZone;
use ical::{col::Occurrence, objects::EventLike};
use std::{
    path::{self, PathBuf},
    process::Command,
    sync::Arc,
};
use tokio::time;
use tracing::warn;

use crate::{
    locale::{DateFlags, Locale},
    settings::Settings,
    state::EventixState,
};

struct Notification {
    pub appname: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub timeout: i32,
}

impl Notification {
    fn from_occurrence(occ: &Occurrence<'_>, locale: &Arc<dyn Locale + Send + Sync>) -> Self {
        let mut body = String::new();
        if let Some(start) = occ.occurrence_start() {
            body = format!("Start: {}", locale.fmt_datetime(&start, DateFlags::None));
        }
        if let Some(loc) = occ.location() {
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
            summary: occ.summary().cloned().unwrap_or(String::from("?")),
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

pub async fn watch_alarms(state: EventixState, locale: Arc<dyn Locale + Send + Sync>) {
    loop {
        {
            let mut state = state.lock().await;
            let last_check = chrono::Utc
                .from_utc_datetime(&state.last_alarm_check())
                .with_timezone(locale.timezone());
            let now = chrono::Utc::now().with_timezone(locale.timezone());

            // find all due alarms since the last check and sort them by alarm time
            let mut alarms = state
                .store()
                .due_alarms_between(last_check, now)
                .filter(|o| !o.is_cancelled())
                .collect::<Vec<_>>();
            alarms.sort_by_key(|o| o.alarm_date().unwrap());

            for alarm in alarms {
                let notification = Notification::from_occurrence(&alarm, &locale);
                notification.send().unwrap();
            }

            state.set_last_alarm_check(now.naive_utc());

            // permanently remember last time of check
            let settings = Settings::new_from_state(&state).await;
            if let Err(e) = settings.write_to_file().await {
                warn!("Unable to save settings: {}", e);
            }
        }

        time::sleep(time::Duration::from_secs(30)).await;
    }
}
