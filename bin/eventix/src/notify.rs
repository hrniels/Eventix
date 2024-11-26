use anyhow::anyhow;
use chrono_tz::Tz;
use ical::{
    col::Occurrence,
    objects::{CalDate, EventLike},
};
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
            body.push_str(&format!("\n  Where: {}", loc));
        }
        body.push_str(&format!(
            "\n  Reminder scheduled for: {}",
            occ.alarm_date().unwrap().format("%H:%M:%S")
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
    // remember the last uid, rid, and last-modified for the last notification we sent. in that
    // way, we also reconsider to send a notification if an event was modified. it of course also
    // means that we might send a notification for the same event multiple times, but I think
    // that's less bad than missing a notification.
    let mut last: Option<(String, Option<CalDate>, Option<CalDate>)> = None;

    loop {
        let now = chrono::Utc::now().with_timezone(&tz);

        let store = state.store().lock().await;
        let next = store.next_alarm_occurrence(now);
        if let Some(next) = next {
            let next_key = Some((
                next.uid().clone(),
                next.rid().cloned(),
                next.last_modified().cloned(),
            ));

            // only send a notification if it's different from last time and the alarm is due
            if next_key != last && next.alarm_date().unwrap() <= now {
                let notification = Notification::from_occurrence(&next);
                notification.send().unwrap();

                last = next_key;
            }
        }
        drop(store);

        time::sleep(time::Duration::from_secs(5)).await;
    }
}
