use anyhow::anyhow;
use chrono::Duration;
use chrono_tz::Tz;
use ical::{col::Occurrence, objects::EventLike};
use std::{
    path::{self, PathBuf},
    process::Command,
};
use tokio::time;

use crate::state::State;

struct Notification {
    pub appname: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub timeout: i32,
}

impl Notification {
    fn from_occurrence(occ: &Occurrence<'_>) -> Self {
        let mut body = format!(
            "Start: {}",
            occ.occurrence_start().format("%A, %b %d %Y, %H:%M:%S")
        );
        if let Some(loc) = occ.location() {
            body.push_str(&format!("\nWhere: {}", loc));
        }
        body.push_str(&format!(
            "\nReminder for: {}",
            occ.alarm_date().unwrap().format("%b %d %Y, %H:%M:%S")
        ));

        let icon: PathBuf = ["static", "images", "icon.png"].iter().collect();
        let icon = path::absolute(icon).unwrap();
        Self {
            appname: String::from("Eventix"),
            icon: icon.into_os_string().into_string().unwrap(),
            summary: occ.summary().cloned().unwrap_or(String::from("?")),
            body,
            timeout: 0,
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

pub async fn watch_alarms(state: State, tz: Tz) {
    // remember the last time we checked for alarms
    let mut last_check = chrono::Utc::now().with_timezone(&tz) - Duration::days(7);

    loop {
        let now = chrono::Utc::now().with_timezone(&tz);

        let store = state.store().lock().await;

        // find all due alarms since the last check and sort them by alarm time
        let mut alarms = store.due_alarms_within(last_check, now).collect::<Vec<_>>();
        alarms.sort_by_key(|o| o.alarm_date().unwrap());

        for alarm in alarms {
            let notification = Notification::from_occurrence(&alarm);
            notification.send().unwrap();
        }

        last_check = now;
        drop(store);

        time::sleep(time::Duration::from_secs(30)).await;
    }
}
