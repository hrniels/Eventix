use anyhow::{Context, anyhow};
use eventix_ical::col::Occurrence;
use eventix_ical::objects::{CalAlarm, CalDate, EventLike};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::read_dir,
    path::PathBuf,
};
use xdg::BaseDirectories;

use crate::CalendarAlarmType;

const ALARMS_DIRECTORY: &str = "alarms";

/// Manages personal alarm overrides across all calendars.
///
/// Stores per-calendar alarm configurations on disk under the XDG data directory, keyed by
/// calendar ID. Each calendar's alarms are persisted as a separate TOML file.
#[derive(Default, Debug, Eq, PartialEq)]
pub struct PersonalAlarms {
    path: PathBuf,
    calendars: BTreeMap<String, PersonalCalendarAlarms>,
}

impl PersonalAlarms {
    /// Loads personal alarms from the XDG data directory.
    ///
    /// Reads all `.toml` files found in the alarms subdirectory. Each file corresponds to a single
    /// calendar, identified by its stem.
    pub fn new_from_dir(xdg: &BaseDirectories) -> anyhow::Result<Self> {
        let path = xdg
            .create_data_directory(ALARMS_DIRECTORY)
            .context("create alarms directory")?;

        let mut calendars = BTreeMap::new();
        let dir_files =
            read_dir(path.as_path()).context(anyhow!("opening directory {:?}", path))?;
        for entry in dir_files {
            let entry = entry.context(anyhow!("reading directory {:?}", path))?;
            if !entry
                .file_type()
                .context(anyhow!(
                    "getting file type of {:?}/{:?}",
                    path,
                    entry.file_name()
                ))?
                .is_file()
            {
                continue;
            }

            let filename = entry.path();
            if filename
                .extension()
                .and_then(|ex| ex.to_str())
                .is_none_or(|ex| ex != "toml")
            {
                continue;
            }

            let cal_id = filename
                .file_stem()
                .context(anyhow!("extracing calendar id from {:?}", filename))?
                .to_str()
                .context(anyhow!("converting to string from {:?}", filename))?
                .to_string();
            let cal_alarms = PersonalCalendarAlarms::new_from_file(filename)?;
            calendars.insert(cal_id, cal_alarms);
        }

        Ok(Self { path, calendars })
    }

    /// Returns the personal alarm configuration for the calendar with the given `id`.
    ///
    /// Returns `None` if no configuration exists for that calendar.
    pub fn get(&self, id: &str) -> Option<&PersonalCalendarAlarms> {
        self.calendars
            .iter()
            .find(|(cid, _cal)| *cid == id)
            .map(|(_cid, cal)| cal)
    }

    /// Returns a mutable reference to the alarm configuration for the calendar with the given `id`.
    ///
    /// Creates an empty in-memory entry if none exists yet.
    pub fn get_or_create(&mut self, id: &str) -> &mut PersonalCalendarAlarms {
        self.calendars.entry(id.to_string()).or_insert_with(|| {
            let mut path = self.path.clone();
            path.push(format!("{id}.toml"));
            PersonalCalendarAlarms::new_empty(path)
        })
    }

    /// Returns whether the given occurrence has at least one effective alarm.
    ///
    /// For `Personal` mode, falls back to the configured default alarm when no per-event override
    /// exists.
    pub fn has_alarms(&self, occ: &Occurrence<'_>, settings: &CalendarAlarmType) -> bool {
        match settings {
            CalendarAlarmType::Personal { default } => {
                if let Some(cal) = self.get(occ.directory()) {
                    cal.has_alarms(occ, default)
                } else {
                    default.is_some()
                }
            }
            CalendarAlarmType::Calendar => occ.has_alarms(),
        }
    }

    /// Returns the effective alarms for the given occurrence, or `None` if no alarm applies.
    ///
    /// For `Personal` mode, per-event overrides take precedence over the default alarm. An empty
    /// override list means alarms are explicitly disabled and yields `None`.
    pub fn effective_alarms(
        &self,
        occ: &Occurrence<'_>,
        alarm_type: &CalendarAlarmType,
    ) -> Option<Vec<CalAlarm>> {
        match alarm_type {
            CalendarAlarmType::Personal { default } => {
                if let Some(cal) = self.get(occ.directory()) {
                    cal.effective_alarms(occ, default)
                } else {
                    default.clone().map(|alarm| vec![alarm])
                }
            }
            CalendarAlarmType::Calendar => occ.alarms().map(|a| a.to_vec()),
        }
    }
}

/// A user-defined alarm override for a specific event or occurrence.
///
/// Associates a set of [`CalAlarm`] values with a particular UID and optional recurrence ID. When
/// `rid` is `None`, the override applies to the base event and serves as the default for all its
/// occurrences that have no more-specific override.
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlarmOverwrite {
    uid: String,
    #[serde(default)]
    rid: Option<CalDate>,
    alarms: Vec<CalAlarm>,
}

impl AlarmOverwrite {
    /// Returns the alarms stored in this override.
    pub fn alarms(&self) -> &[CalAlarm] {
        &self.alarms
    }
}

/// Personal alarm overrides for a single calendar, persisted as a TOML file.
///
/// Each entry overrides the alarms for a specific event UID, optionally scoped to a single
/// occurrence via a recurrence ID. An empty alarm list in an entry explicitly disables all alarms
/// for that event or occurrence.
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PersonalCalendarAlarms {
    #[serde(skip)]
    path: PathBuf,
    alarms: Vec<AlarmOverwrite>,
}

impl PersonalCalendarAlarms {
    fn new_empty(path: PathBuf) -> Self {
        Self {
            path,
            alarms: Vec::default(),
        }
    }

    /// Loads a `PersonalCalendarAlarms` from a TOML file at the given path.
    pub fn new_from_file(path: PathBuf) -> anyhow::Result<Self> {
        super::load_from_file::<Self>(&path).map(|res| Self {
            path,
            alarms: res.alarms,
        })
    }

    /// Returns whether the given occurrence has at least one effective alarm.
    ///
    /// Falls back to the `default` alarm when no per-event override exists.
    pub fn has_alarms(&self, occ: &Occurrence<'_>, default: &Option<CalAlarm>) -> bool {
        match self.get(occ.uid(), Self::occurrence_rid(occ).as_ref()) {
            Some(overwrite) => !overwrite.alarms().is_empty(),
            None => default.is_some(),
        }
    }

    /// Returns the effective alarms for the given occurrence, or `None` if no alarm applies.
    ///
    /// An empty override list means alarms are explicitly disabled for that occurrence and this
    /// method returns `None`. Falls back to `default` when no override exists.
    pub fn effective_alarms(
        &self,
        occ: &Occurrence<'_>,
        default: &Option<CalAlarm>,
    ) -> Option<Vec<CalAlarm>> {
        match self.get(occ.uid(), Self::occurrence_rid(occ).as_ref()) {
            Some(overwrite) => {
                // an empty alarm list here means that we have overwritten it to disable all
                // alarms, but the caller expects this as None.
                if overwrite.alarms().is_empty() {
                    None
                } else {
                    Some(overwrite.alarms().to_vec())
                }
            }
            None => default.clone().map(|alarm| vec![alarm]),
        }
    }

    fn occurrence_rid(occ: &Occurrence<'_>) -> Option<CalDate> {
        // if we have an overwrite for this occurrence, use its rid because the start date might be
        // overwritten.
        if occ.is_overwritten() {
            occ.rid().cloned()
        }
        // if there is no overwrite, we have no rid and thus have to use the start date (which also
        // cannot have been changed here).
        else {
            occ.occurrence_startdate().map(|d| d.to_utc())
        }
    }

    /// Returns a map from recurrence ID to alarm list for all occurrence-level overrides for `uid`.
    ///
    /// Entries without a recurrence ID (base-event overrides) are excluded.
    pub fn all_for_occurrences(&self, uid: &str) -> HashMap<CalDate, Vec<CalAlarm>> {
        let mut res = HashMap::new();
        for a in &self.alarms {
            if a.uid == uid
                && let Some(rid) = &a.rid
            {
                res.insert(rid.clone(), a.alarms().to_vec());
            }
        }
        res
    }

    /// Returns the alarm override for the event identified by `uid` and optional `rid`.
    ///
    /// When an occurrence-level lookup (non-`None` `rid`) finds no entry, falls back to the
    /// base-event override (where `rid` is `None`), if one exists.
    pub fn get(&self, uid: &str, rid: Option<&CalDate>) -> Option<&AlarmOverwrite> {
        let overwrite = self
            .alarms
            .iter()
            .find(|a| a.uid == uid && a.rid.as_ref() == rid);
        if overwrite.is_some() {
            return overwrite;
        }

        // if it's an occurrence and there is an overwrite for the base component, use that
        if rid.is_some() {
            self.alarms.iter().find(|a| a.uid == uid && a.rid.is_none())
        } else {
            None
        }
    }

    /// Sets the alarm override for the event identified by `uid` and optional `rid`.
    ///
    /// Returns `true` if the stored data changed and a subsequent [`save`](Self::save) is needed,
    /// or `false` if the entry was redundant (the alarms match the base-event override) and was
    /// removed or not stored.
    pub fn set(&mut self, uid: &str, rid: Option<&CalDate>, alarms: Vec<CalAlarm>) -> bool {
        // if it's an occurrence and the alarms are the same as for the base component, we don't
        // need to store it for the occurrence
        if rid.is_some() {
            let base = self.alarms.iter().find(|a| a.uid == uid && a.rid.is_none());
            if let Some(base) = base
                && base.alarms() == alarms
            {
                // remove the old setting, if there was one
                return self.unset(uid, rid);
            }
        }

        if let Some(ex_alarm) = self
            .alarms
            .iter_mut()
            .find(|a| a.uid == uid && a.rid.as_ref() == rid)
        {
            ex_alarm.alarms = alarms;
        } else {
            self.alarms.push(AlarmOverwrite {
                uid: uid.to_string(),
                rid: rid.cloned(),
                alarms,
            });
        }
        true
    }

    /// Removes the alarm override for the event identified by `uid` and optional `rid`.
    ///
    /// Returns `true` if an entry was found and removed, `false` if no matching entry existed.
    pub fn unset(&mut self, uid: &str, rid: Option<&CalDate>) -> bool {
        let len = self.alarms.len();
        self.alarms
            .retain(|a| a.uid != uid || a.rid.as_ref() != rid);
        self.alarms.len() != len
    }

    /// Persists the current alarm overrides to the TOML file at the path associated with this
    /// instance.
    pub fn save(&self) -> anyhow::Result<()> {
        super::write_to_file(&self.path, self)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeDelta};
    use eventix_ical::objects::{
        CalAction, CalAlarm, CalDate, CalDateType, CalLocaleEn, CalRelated, CalTrigger,
    };

    use super::PersonalCalendarAlarms;

    #[test]
    fn basics() {
        let mut alarms = PersonalCalendarAlarms::new_empty("".into());
        assert_eq!(alarms.get("test", None), None);

        assert!(alarms.set("test", None, vec![]));

        let alarm = alarms.get("test", None).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid, None);
        assert_eq!(alarm.alarms, vec![]);

        assert!(alarms.unset("test", None));
        assert_eq!(alarms.get("test", None), None);
    }

    #[test]
    fn with_rid() {
        let rid1 = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 10, 1).unwrap(),
            CalDateType::Inclusive,
        );
        let mut alarms = PersonalCalendarAlarms::new_empty("".into());

        assert!(alarms.set("test", Some(&rid1), vec![]));

        assert_eq!(alarms.get("test", None), None);
        let alarm = alarms.get("test", Some(&rid1)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid.as_ref(), Some(&rid1));
        assert_eq!(alarm.alarms, vec![]);

        assert!(alarms.unset("test", Some(&rid1)));
        assert_eq!(alarms.get("test", Some(&rid1)), None);
    }

    #[test]
    fn with_rid_inheritance() {
        let rid1 = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 10, 1).unwrap(),
            CalDateType::Inclusive,
        );
        let rid2 = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 10, 2).unwrap(),
            CalDateType::Inclusive,
        );
        let alarm = CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                related: CalRelated::Start,
                duration: TimeDelta::minutes(5).into(),
            },
        );
        let mut alarms = PersonalCalendarAlarms::new_empty("".into());

        assert!(alarms.set("test", None, vec![alarm.clone()]));
        assert!(alarms.set("test", Some(&rid1), vec![]));

        assert_eq!(alarms.alarms.len(), 2);
        assert!(!alarms.set("test", Some(&rid2), vec![alarm.clone()]));
        assert_eq!(alarms.alarms.len(), 2);

        let alarm = alarms.get("test", Some(&rid1)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid.as_ref(), Some(&rid1));
        assert_eq!(alarm.alarms, vec![]);

        let alarm = alarms.get("test", None).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid, None);
        assert_eq!(
            format!("{}", alarm.alarms[0].human(&CalLocaleEn)),
            "5 minutes after start"
        );

        let alarm = alarms.get("test", Some(&rid2)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid, None);

        assert!(!alarms.unset("test", Some(&rid2)));

        let alarm = alarms.get("test", Some(&rid2)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid, None);

        assert!(alarms.unset("test", None));
        assert_eq!(alarms.get("test", Some(&rid2)), None);
        let alarm = alarms.get("test", Some(&rid1)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid.as_ref(), Some(&rid1));
    }
}
