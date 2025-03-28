use anyhow::{anyhow, Context};
use ical::{
    col::Occurrence,
    objects::{CalAlarm, CalDate, EventLike},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{create_dir_all, read_dir},
    path::PathBuf,
};

const ALARMS_DIRECTORY: &str = "data/alarms";

#[derive(Default, Debug, Eq, PartialEq)]
pub struct PersonalAlarms {
    path: PathBuf,
    calendars: BTreeMap<String, PersonalCalendarAlarms>,
}

impl PersonalAlarms {
    pub fn new_from_dir() -> anyhow::Result<Self> {
        // ensure all directories exist
        let path: PathBuf = ALARMS_DIRECTORY.into();
        create_dir_all(&path)?;

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

    pub fn get(&self, id: &str) -> Option<&PersonalCalendarAlarms> {
        self.calendars
            .iter()
            .find(|(cid, _cal)| *cid == id)
            .map(|(_cid, cal)| cal)
    }

    pub fn get_or_create(&mut self, id: &str) -> &mut PersonalCalendarAlarms {
        self.calendars.entry(id.to_string()).or_insert_with(|| {
            let mut path = self.path.clone();
            path.push(format!("{}.toml", id));
            PersonalCalendarAlarms::new_empty(path)
        })
    }

    pub fn has_alarms(&self, occ: &Occurrence<'_>) -> bool {
        if let Some(cal) = self.get(occ.directory()) {
            cal.has_alarms(occ)
        } else {
            occ.has_alarms()
        }
    }

    pub fn effective_alarms(&self, occ: &Occurrence<'_>) -> Option<Vec<CalAlarm>> {
        if let Some(cal) = self.get(occ.directory()) {
            cal.effective_alarms(occ)
        } else {
            occ.alarms().map(|a| a.to_vec())
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlarmOverwrite {
    uid: String,
    #[serde(
        serialize_with = "serde_impl::serialize_date",
        deserialize_with = "serde_impl::deserialize_date",
        default
    )]
    rid: Option<CalDate>,
    alarms: Vec<CalAlarm>,
}

impl AlarmOverwrite {
    pub fn alarms(&self) -> &[CalAlarm] {
        &self.alarms
    }
}

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

    pub fn new_from_file(path: PathBuf) -> anyhow::Result<Self> {
        super::load_from_file::<Self>(&path).map(|res| Self {
            path,
            alarms: res.alarms,
        })
    }

    pub fn has_alarms(&self, occ: &Occurrence<'_>) -> bool {
        match self.get(occ.uid(), occ.rid()) {
            Some(overwrite) => !overwrite.alarms().is_empty(),
            None => occ.has_alarms(),
        }
    }

    pub fn effective_alarms(&self, occ: &Occurrence<'_>) -> Option<Vec<CalAlarm>> {
        match self.get(occ.uid(), occ.rid()) {
            Some(overwrite) => {
                // an empty alarm list here means that we have overwritten it to disable all
                // alarms, but the caller expects this as None.
                if overwrite.alarms().is_empty() {
                    None
                } else {
                    Some(overwrite.alarms().to_vec())
                }
            }
            None => occ.alarms().map(|a| a.to_vec()),
        }
    }

    pub fn all_for_occurrences(&self, uid: &str) -> HashMap<CalDate, Vec<CalAlarm>> {
        let mut res = HashMap::new();
        for a in &self.alarms {
            if a.uid == uid {
                if let Some(rid) = &a.rid {
                    res.insert(rid.clone(), a.alarms().to_vec());
                }
            }
        }
        res
    }

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

    pub fn set(&mut self, uid: &str, rid: Option<&CalDate>, alarms: Vec<CalAlarm>) -> bool {
        // if it's an occurrence and the alarms are the same as for the base component, we don't
        // need to store it for the occurrence
        if rid.is_some() {
            let base = self.alarms.iter().find(|a| a.uid == uid && a.rid.is_none());
            if let Some(base) = base {
                if base.alarms() == alarms {
                    // remove the old setting, if there was one
                    return self.unset(uid, rid);
                }
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

    pub fn unset(&mut self, uid: &str, rid: Option<&CalDate>) -> bool {
        let len = self.alarms.len();
        self.alarms
            .retain(|a| a.uid != uid || a.rid.as_ref() != rid);
        self.alarms.len() != len
    }

    pub fn save(&self) -> anyhow::Result<()> {
        super::write_to_file(&self.path, self)
    }
}

mod serde_impl {
    use super::*;

    pub fn serialize_date<S>(date: &Option<CalDate>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match date {
            Some(date) => serializer.serialize_some(&format!("{}", date)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize_date<'de, D>(deserializer: D) -> Result<Option<CalDate>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        if buf.is_empty() {
            Ok(None)
        } else {
            Ok(Some(buf.parse().map_err(serde::de::Error::custom)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeDelta};
    use chrono_tz::Tz;
    use ical::objects::{CalAction, CalAlarm, CalDate, CalDateType, CalRelated, CalTrigger};

    use super::PersonalCalendarAlarms;

    #[test]
    fn basics() {
        let mut alarms = PersonalCalendarAlarms::new_empty("".into());
        assert_eq!(alarms.get("test", None), None);

        assert_eq!(alarms.set("test", None, vec![]), true);

        let alarm = alarms.get("test", None).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid, None);
        assert_eq!(alarm.alarms, vec![]);

        assert_eq!(alarms.unset("test", None), true);
        assert_eq!(alarms.get("test", None), None);
    }

    #[test]
    fn with_rid() {
        let rid1 = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 10, 1).unwrap(),
            CalDateType::Inclusive,
        );
        let mut alarms = PersonalCalendarAlarms::new_empty("".into());

        assert_eq!(alarms.set("test", Some(&rid1), vec![]), true);

        assert_eq!(alarms.get("test", None), None);
        let alarm = alarms.get("test", Some(&rid1)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid.as_ref(), Some(&rid1));
        assert_eq!(alarm.alarms, vec![]);

        assert_eq!(alarms.unset("test", Some(&rid1)), true);
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
                duration: TimeDelta::minutes(5),
            },
        );
        let mut alarms = PersonalCalendarAlarms::new_empty("".into());

        assert_eq!(alarms.set("test", None, vec![alarm.clone()]), true);
        assert_eq!(alarms.set("test", Some(&rid1), vec![]), true);

        assert_eq!(alarms.alarms.len(), 2);
        assert_eq!(alarms.set("test", Some(&rid2), vec![alarm.clone()]), false);
        assert_eq!(alarms.alarms.len(), 2);

        let alarm = alarms.get("test", Some(&rid1)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid.as_ref(), Some(&rid1));
        assert_eq!(alarm.alarms, vec![]);

        let alarm = alarms.get("test", None).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid, None);
        assert_eq!(
            format!("{}", alarm.alarms[0].human(&Tz::UTC)),
            "5 minutes after start"
        );

        let alarm = alarms.get("test", Some(&rid2)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid, None);

        assert_eq!(alarms.unset("test", Some(&rid2)), false);

        let alarm = alarms.get("test", Some(&rid2)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid, None);

        assert_eq!(alarms.unset("test", None), true);
        assert_eq!(alarms.get("test", Some(&rid2)), None);
        let alarm = alarms.get("test", Some(&rid1)).unwrap();
        assert_eq!(alarm.uid, "test");
        assert_eq!(alarm.rid.as_ref(), Some(&rid1));
    }
}
