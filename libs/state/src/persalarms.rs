// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
    use std::sync::Arc;

    use chrono::{NaiveDate, TimeDelta, TimeZone};
    use chrono_tz::UTC;
    use eventix_ical::col::Occurrence;
    use eventix_ical::objects::{
        CalAction, CalAlarm, CalComponent, CalDate, CalDateType, CalEvent, CalLocaleEn, CalRelated,
        CalTrigger, UpdatableEventLike,
    };

    use super::{PersonalAlarms, PersonalCalendarAlarms};

    // --- helpers ---

    fn make_alarm() -> CalAlarm {
        CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                related: CalRelated::Start,
                duration: TimeDelta::minutes(5).into(),
            },
        )
    }

    /// Builds a simple timed `Occurrence` with the given `uid` rooted in `dir_id`.
    ///
    /// The occurrence has a known start date so that `occurrence_startdate()` returns a value,
    /// which is what `PersonalCalendarAlarms::occurrence_rid` uses for non-overwritten occurrences.
    fn make_occurrence<'c>(dir_id: &Arc<String>, comp: &'c CalComponent) -> Occurrence<'c> {
        let start = UTC.with_ymd_and_hms(2024, 6, 1, 9, 0, 0).unwrap();
        Occurrence::new(
            dir_id.clone(),
            comp,
            Some(start.fixed_offset().into()),
            None,
            false,
        )
    }

    /// Builds a `CalComponent::Event` with the given `uid` and no alarms set.
    fn make_comp(uid: &str) -> CalComponent {
        CalComponent::Event(CalEvent::new(uid))
    }

    /// Builds a `CalComponent::Event` that carries one `CalAlarm`.
    fn make_comp_with_alarm(uid: &str) -> CalComponent {
        let mut ev = CalEvent::new(uid);
        ev.set_alarms(Some(vec![make_alarm()]));
        CalComponent::Event(ev)
    }

    /// Builds an occurrence whose `occurrence_startdate()` maps to the given `CalDate`.
    ///
    /// The returned `CalDate` can be used directly as the `rid` key in
    /// `PersonalCalendarAlarms::set` / `get`.
    fn occurrence_rid_for_start(dir_id: &Arc<String>, comp: &CalComponent) -> CalDate {
        let occ = make_occurrence(dir_id, comp);
        occ.occurrence_startdate()
            .expect("occurrence must have a start date")
    }

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

    // --- set: update existing entry ---

    /// Verifies that `set` updates the alarms on an already-stored entry rather than adding a
    /// duplicate.
    #[test]
    fn set_updates_existing_entry() {
        let alarm = make_alarm();
        let mut cal_alarms = PersonalCalendarAlarms::new_empty("".into());

        // Store an initial entry (empty alarm list).
        assert!(cal_alarms.set("uid-upd", None, vec![]));
        assert_eq!(cal_alarms.alarms.len(), 1);

        // Overwrite with a non-empty alarm list — must update the existing entry, not append.
        assert!(cal_alarms.set("uid-upd", None, vec![alarm.clone()]));
        assert_eq!(cal_alarms.alarms.len(), 1);
        assert_eq!(cal_alarms.get("uid-upd", None).unwrap().alarms(), &[alarm]);
    }

    // --- PersonalCalendarAlarms: has_alarms and effective_alarms ---

    /// Verifies `has_alarms` and `effective_alarms` when no override exists for the occurrence.
    ///
    /// Without an override the result depends entirely on whether a `default` alarm is present.
    #[test]
    fn has_and_effective_alarms_no_override() {
        let dir_id = Arc::new("cal-a".to_string());
        let comp = make_comp("uid-no-override");
        let occ = make_occurrence(&dir_id, &comp);

        let cal_alarms = PersonalCalendarAlarms::new_empty("".into());

        // No override, no default → no alarm.
        assert!(!cal_alarms.has_alarms(&occ, &None));
        assert_eq!(cal_alarms.effective_alarms(&occ, &None), None);

        // No override but default present → default alarm is returned.
        let default = make_alarm();
        assert!(cal_alarms.has_alarms(&occ, &Some(default.clone())));
        assert_eq!(
            cal_alarms.effective_alarms(&occ, &Some(default.clone())),
            Some(vec![default]),
        );
    }

    /// Verifies `has_alarms` and `effective_alarms` when an override with alarms is present.
    #[test]
    fn has_and_effective_alarms_with_override() {
        let dir_id = Arc::new("cal-b".to_string());
        let comp = make_comp("uid-override");
        let rid = occurrence_rid_for_start(&dir_id, &comp);

        let alarm = make_alarm();
        let mut cal_alarms = PersonalCalendarAlarms::new_empty("".into());
        cal_alarms.set("uid-override", Some(&rid), vec![alarm.clone()]);

        let occ = make_occurrence(&dir_id, &comp);

        // Override present with one alarm → has_alarms true, alarms returned regardless of default.
        assert!(cal_alarms.has_alarms(&occ, &None));
        assert_eq!(
            cal_alarms.effective_alarms(&occ, &None),
            Some(vec![alarm.clone()]),
        );
        // Default is ignored when an override exists.
        let other_default = CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                related: CalRelated::Start,
                duration: TimeDelta::minutes(10).into(),
            },
        );
        assert_eq!(
            cal_alarms.effective_alarms(&occ, &Some(other_default)),
            Some(vec![alarm]),
        );
    }

    /// Verifies that an empty override list explicitly disables alarms for the occurrence.
    #[test]
    fn has_and_effective_alarms_empty_override_disables() {
        let dir_id = Arc::new("cal-c".to_string());
        let comp = make_comp("uid-disabled");
        let rid = occurrence_rid_for_start(&dir_id, &comp);

        let mut cal_alarms = PersonalCalendarAlarms::new_empty("".into());
        cal_alarms.set("uid-disabled", Some(&rid), vec![]);

        let occ = make_occurrence(&dir_id, &comp);

        // Empty override → explicitly no alarms, even if a default is set.
        assert!(!cal_alarms.has_alarms(&occ, &Some(make_alarm())));
        assert_eq!(cal_alarms.effective_alarms(&occ, &Some(make_alarm())), None);
    }

    // --- PersonalCalendarAlarms: all_for_occurrences ---

    /// Verifies `all_for_occurrences` returns only occurrence-level entries (rid is Some) and
    /// excludes base-event overrides (rid is None).
    #[test]
    fn all_for_occurrences() {
        let rid1 = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 11, 1).unwrap(),
            CalDateType::Inclusive,
        );
        let rid2 = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 11, 2).unwrap(),
            CalDateType::Inclusive,
        );
        let alarm = make_alarm();
        let alarm2 = CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                related: CalRelated::Start,
                duration: TimeDelta::minutes(10).into(),
            },
        );

        let mut cal_alarms = PersonalCalendarAlarms::new_empty("".into());
        // Base-event override (no rid) — must not appear in all_for_occurrences.
        cal_alarms.set("uid-afo", None, vec![alarm.clone()]);
        // Two occurrence-level overrides with alarms distinct from the base, so neither is
        // collapsed into the base-event override by `set`.
        cal_alarms.set("uid-afo", Some(&rid1), vec![alarm2.clone()]);
        cal_alarms.set("uid-afo", Some(&rid2), vec![]);

        let map = cal_alarms.all_for_occurrences("uid-afo");
        assert_eq!(map.len(), 2);
        assert_eq!(map[&rid1], vec![alarm2]);
        assert_eq!(map[&rid2], vec![]);

        // A uid with no overrides at all yields an empty map.
        assert!(cal_alarms.all_for_occurrences("uid-none").is_empty());

        // A uid whose only override is at the base level also yields an empty map.
        cal_alarms.set("uid-base-only", None, vec![alarm.clone()]);
        assert!(cal_alarms.all_for_occurrences("uid-base-only").is_empty());
    }

    // --- PersonalCalendarAlarms: save and new_from_file ---

    /// Verifies that `save` writes a TOML file and `new_from_file` can read it back unchanged.
    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cal-rt.toml");

        let rid = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 12, 1).unwrap(),
            CalDateType::Inclusive,
        );
        let alarm = make_alarm();

        let mut original = PersonalCalendarAlarms::new_empty(path.clone());
        original.set("uid-rt", None, vec![alarm.clone()]);
        original.set("uid-rt", Some(&rid), vec![]);
        original.save().expect("save must succeed");

        let loaded = PersonalCalendarAlarms::new_from_file(path).expect("load must succeed");

        // Base-event override survives round-trip.
        let base = loaded.get("uid-rt", None).unwrap();
        assert_eq!(base.uid, "uid-rt");
        assert_eq!(base.alarms(), &[alarm]);

        // Occurrence override survives round-trip.
        let occ_entry = loaded.get("uid-rt", Some(&rid)).unwrap();
        assert_eq!(occ_entry.rid.as_ref(), Some(&rid));
        assert_eq!(occ_entry.alarms(), &[] as &[CalAlarm]);
    }

    // --- PersonalAlarms ---

    /// Verifies `PersonalAlarms::get` and `get_or_create` without touching the filesystem.
    #[test]
    fn personal_alarms_get_and_get_or_create() {
        // new_from_dir requires a real XDG environment; exercise the public API via the default
        // (empty) value and direct map manipulation instead.
        let mut pa = PersonalAlarms::default();

        // Nothing stored yet.
        assert!(pa.get("cal-x").is_none());

        // get_or_create inserts an empty entry and returns a mutable ref.
        {
            let entry = pa.get_or_create("cal-x");
            entry.set("uid-pa", None, vec![]);
        }

        // Now get returns the stored entry.
        let entry = pa.get("cal-x").unwrap();
        assert!(entry.get("uid-pa", None).is_some());

        // get_or_create on an existing key returns the same entry.
        let entry2 = pa.get_or_create("cal-x");
        assert!(entry2.get("uid-pa", None).is_some());
    }

    /// Verifies `PersonalAlarms::new_from_dir` loads TOML files from an on-disk alarms directory.
    #[test]
    fn personal_alarms_new_from_dir() {
        let tmpdir = tempfile::tempdir().unwrap();
        let alarms_dir = tmpdir.path().join("alarms");
        std::fs::create_dir_all(&alarms_dir).unwrap();

        // Write a minimal TOML file for calendar "my-cal".
        let alarm = make_alarm();
        let cal_path = alarms_dir.join("my-cal.toml");
        let mut stored = PersonalCalendarAlarms::new_empty(cal_path.clone());
        stored.set("uid-dir", None, vec![alarm.clone()]);
        stored.save().expect("save must succeed");

        // Also create a non-.toml file and a subdirectory — both must be ignored.
        std::fs::write(alarms_dir.join("ignored.txt"), b"ignored").unwrap();
        std::fs::create_dir(alarms_dir.join("subdir")).unwrap();

        // Point XDG data home at the temp directory so that `create_data_directory("alarms")`
        // resolves to our prepared directory.
        let xdg = crate::with_test_xdg(tmpdir.path(), tmpdir.path());
        let pa = PersonalAlarms::new_from_dir(&xdg).expect("new_from_dir must succeed");

        let cal = pa.get("my-cal").expect("my-cal must be loaded");
        let entry = cal.get("uid-dir", None).unwrap();
        assert_eq!(entry.alarms(), &[alarm]);

        // The non-toml file and subdirectory were not loaded.
        assert!(pa.get("ignored").is_none());
        assert!(pa.get("subdir").is_none());
    }

    /// Verifies `PersonalAlarms::has_alarms` and `effective_alarms` for the `Calendar` variant.
    ///
    /// In `Calendar` mode the result comes directly from the occurrence's embedded alarms, not
    /// from any personal override.
    #[test]
    fn personal_alarms_calendar_mode() {
        let dir_id = Arc::new("cal-cal".to_string());

        // Occurrence whose underlying event has an alarm.
        let comp_with = make_comp_with_alarm("uid-cal-mode");
        let occ_with = make_occurrence(&dir_id, &comp_with);

        // Occurrence whose underlying event has no alarm.
        let comp_without = make_comp("uid-no-alarm");
        let occ_without = make_occurrence(&dir_id, &comp_without);

        let pa = PersonalAlarms::default();
        let alarm_type = crate::CalendarAlarmType::Calendar;

        assert!(pa.has_alarms(&occ_with, &alarm_type));
        assert!(!pa.has_alarms(&occ_without, &alarm_type));

        // effective_alarms returns the occurrence's own alarm list.
        let alarms = pa.effective_alarms(&occ_with, &alarm_type).unwrap();
        assert_eq!(alarms.len(), 1);
        assert_eq!(pa.effective_alarms(&occ_without, &alarm_type), None);
    }

    /// Verifies `PersonalAlarms::has_alarms` and `effective_alarms` for `Personal` mode when the
    /// calendar has no stored overrides (falls back to the default alarm).
    #[test]
    fn personal_alarms_personal_mode_no_calendar_entry() {
        let dir_id = Arc::new("cal-unknown".to_string());
        let comp = make_comp("uid-unknown-cal");
        let occ = make_occurrence(&dir_id, &comp);

        let pa = PersonalAlarms::default();

        let no_default = crate::CalendarAlarmType::Personal { default: None };
        assert!(!pa.has_alarms(&occ, &no_default));
        assert_eq!(pa.effective_alarms(&occ, &no_default), None);

        let default_alarm = make_alarm();
        let with_default = crate::CalendarAlarmType::Personal {
            default: Some(default_alarm.clone()),
        };
        assert!(pa.has_alarms(&occ, &with_default));
        assert_eq!(
            pa.effective_alarms(&occ, &with_default),
            Some(vec![default_alarm]),
        );
    }

    /// Verifies `PersonalAlarms::has_alarms` and `effective_alarms` for `Personal` mode when the
    /// calendar does have stored overrides (delegates to `PersonalCalendarAlarms`).
    #[test]
    fn personal_alarms_personal_mode_with_calendar_entry() {
        let dir_id = Arc::new("cal-known".to_string());
        let comp = make_comp("uid-known-cal");
        let rid = occurrence_rid_for_start(&dir_id, &comp);

        let alarm = make_alarm();
        let mut pa = PersonalAlarms::default();
        {
            let entry = pa.get_or_create("cal-known");
            entry.set("uid-known-cal", Some(&rid), vec![alarm.clone()]);
        }

        let occ = make_occurrence(&dir_id, &comp);
        let alarm_type = crate::CalendarAlarmType::Personal { default: None };

        assert!(pa.has_alarms(&occ, &alarm_type));
        assert_eq!(pa.effective_alarms(&occ, &alarm_type), Some(vec![alarm]),);
    }

    /// Verifies `occurrence_rid` uses the overwrite's `rid` when the occurrence is overwritten.
    ///
    /// When `is_overwritten()` is true the private `occurrence_rid` helper reads `occ.rid()` from
    /// the overwrite component rather than deriving it from `occurrence_startdate`. This test
    /// confirms that alarm lookups work correctly in that scenario.
    #[test]
    fn occurrence_rid_uses_overwrite_rid() {
        let dir_id = Arc::new("cal-ow".to_string());
        let alarm = make_alarm();

        // The rid stored in the overwrite component.
        let rid = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 9, 15).unwrap(),
            CalDateType::Inclusive,
        );

        // Build base component with no alarms.
        let comp_base = make_comp("uid-ow");

        // Build overwrite component that carries the rid.
        let mut ev_ow = CalEvent::new("uid-ow");
        ev_ow.set_rid(Some(rid.clone()));
        let comp_ow = CalComponent::Event(ev_ow);

        // Create a timed occurrence and attach the overwrite.
        let start = UTC.with_ymd_and_hms(2024, 9, 15, 9, 0, 0).unwrap();
        let mut occ = Occurrence::new(
            dir_id.clone(),
            &comp_base,
            Some(start.fixed_offset().into()),
            None,
            false,
        );
        occ.set_overwrite(
            &comp_ow,
            &chrono_tz::Tz::UTC,
            &eventix_ical::objects::CalendarTimeZoneResolver::new(
                &eventix_ical::objects::Calendar::default(),
            ),
        );

        // Store an alarm override keyed by the overwrite's rid.
        let mut cal_alarms = PersonalCalendarAlarms::new_empty("".into());
        cal_alarms.set("uid-ow", Some(&rid), vec![alarm.clone()]);

        // Lookup must find the override via the overwrite rid path (line 220).
        assert!(cal_alarms.has_alarms(&occ, &None));
        assert_eq!(cal_alarms.effective_alarms(&occ, &None), Some(vec![alarm]),);
    }
}
