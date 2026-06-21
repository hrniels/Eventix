// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use askama::Template;
use axum::http::Uri;
use email_address::EmailAddress;
use eventix_locale::Locale;
use eventix_state::{
    EmailAccount, EncryptedPassword, SyncTimeBound, SyncTimeSpan, SyncerType, encrypt_password,
    retrieve_portal_secret,
};
use formatx::formatx;
use serde::{Deserialize, Deserializer, de};
use std::fmt::{self, Display};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use crate::comps::password::{PasswordRequest, PasswordTemplate};
use crate::html::filters;
use crate::pages::Page;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Syncer {
    #[allow(clippy::enum_variant_names)]
    VDirSyncer,
    O365,
    FileSystem,
}

impl Display for Syncer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Syncer::VDirSyncer => write!(f, "VDIRSYNCER"),
            Syncer::O365 => write!(f, "O365"),
            Syncer::FileSystem => write!(f, "FILESYSTEM"),
        }
    }
}

impl From<&SyncerType> for Syncer {
    fn from(value: &SyncerType) -> Self {
        match value {
            SyncerType::VDirSyncer { .. } => Self::VDirSyncer,
            SyncerType::O365 { .. } => Self::O365,
            SyncerType::FileSystem { .. } => Self::FileSystem,
        }
    }
}

impl Syncer {
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Syncer>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        match buf.as_str() {
            "VDIRSYNCER" => Ok(Some(Syncer::VDirSyncer)),
            "O365" => Ok(Some(Syncer::O365)),
            "FILESYSTEM" => Ok(Some(Syncer::FileSystem)),
            _ => Ok(None),
        }
    }
}

const DEFAULT_TIME_SPAN_YEARS: u32 = 5;
const MAX_TIME_SPAN_YEARS: u32 = 100;

fn default_time_span_years() -> u32 {
    DEFAULT_TIME_SPAN_YEARS
}

#[derive(Debug, Deserialize)]
pub struct SyncerRequest {
    #[serde(deserialize_with = "Syncer::deserialize")]
    syncer: Option<Syncer>,
    vdir_name: String,
    vdir_email: String,
    vdir_url: String,
    vdir_readonly: Option<String>,
    vdir_username: String,
    vdir_pw: PasswordRequest,
    vdir_time_span: String,
    #[serde(
        deserialize_with = "SyncerRequest::deserialize_years",
        default = "default_time_span_years"
    )]
    vdir_time_span_years: u32,
    o365_name: String,
    o365_email: String,
    o365_readonly: Option<String>,
    o365_time_span: String,
    #[serde(
        deserialize_with = "SyncerRequest::deserialize_years",
        default = "default_time_span_years"
    )]
    o365_time_span_years: u32,
    fs_path: String,
}

impl Default for SyncerRequest {
    fn default() -> Self {
        Self {
            syncer: None,
            vdir_name: String::new(),
            vdir_email: String::new(),
            vdir_url: String::new(),
            vdir_readonly: None,
            vdir_username: String::new(),
            vdir_pw: PasswordRequest::default(),
            vdir_time_span: String::new(),
            vdir_time_span_years: DEFAULT_TIME_SPAN_YEARS,
            o365_name: String::new(),
            o365_email: String::new(),
            o365_readonly: None,
            o365_time_span: String::new(),
            o365_time_span_years: DEFAULT_TIME_SPAN_YEARS,
            fs_path: String::new(),
        }
    }
}

impl SyncerRequest {
    pub fn new() -> Self {
        Self {
            syncer: Some(Syncer::VDirSyncer),
            ..Default::default()
        }
    }

    pub fn new_from_syncer(syncer: &SyncerType) -> Self {
        let mut sync = Self {
            syncer: Some(syncer.into()),
            ..Default::default()
        };

        match syncer {
            SyncerType::VDirSyncer {
                email,
                url,
                read_only,
                username,
                password: _,
                time_span,
            } => {
                sync.vdir_name = email.name().clone();
                sync.vdir_email = email.org_address().clone();
                sync.vdir_readonly = match *read_only {
                    true => Some(String::new()),
                    false => None,
                };
                sync.vdir_url = url.clone();
                sync.vdir_username = username.clone().unwrap_or_default();
                (sync.vdir_time_span, sync.vdir_time_span_years) =
                    Self::time_span_to_fields(time_span);
            }

            SyncerType::O365 {
                email,
                read_only,
                password: _,
                time_span,
            } => {
                sync.o365_name = email.name().clone();
                sync.o365_email = email.org_address().clone();
                sync.o365_readonly = match *read_only {
                    true => Some(String::new()),
                    false => None,
                };
                (sync.o365_time_span, sync.o365_time_span_years) =
                    Self::time_span_to_fields(time_span);
            }

            SyncerType::FileSystem { path } => {
                sync.fs_path = path.clone();
            }
        }

        sync
    }

    pub fn syncer(&self) -> Option<Syncer> {
        self.syncer
    }

    pub fn check(
        &self,
        page: &mut Page,
        locale: &Arc<dyn Locale + Send + Sync>,
        is_add: bool,
    ) -> bool {
        let syncer = self.syncer.as_ref().unwrap();
        match syncer {
            Syncer::VDirSyncer => {
                if self.vdir_name.is_empty() {
                    page.add_error(locale.translate("error.collection_your_name"));
                    return false;
                }
                if !EmailAddress::is_valid(&self.vdir_email) {
                    page.add_error(locale.translate("error.collection_your_email"));
                    return false;
                }
                if let Err(e) = self.vdir_url.parse::<Uri>() {
                    page.add_error(
                        formatx!(locale.translate("error.collection_location"), e).unwrap(),
                    );
                    return false;
                }
                if self.vdir_time_span == "years" && self.vdir_time_span_years > MAX_TIME_SPAN_YEARS
                {
                    page.add_error(locale.translate("error.collection_time_span_years"));
                    return false;
                }

                if !self.vdir_username.is_empty() && !self.vdir_pw.check(locale, page, is_add) {
                    return false;
                }
                true
            }
            Syncer::O365 => {
                if self.o365_name.is_empty() {
                    page.add_error(locale.translate("error.collection_your_name"));
                    return false;
                }
                if !EmailAddress::is_valid(&self.o365_email) {
                    page.add_error(locale.translate("error.collection_your_email"));
                    return false;
                }
                if self.o365_time_span == "years" && self.o365_time_span_years > MAX_TIME_SPAN_YEARS
                {
                    page.add_error(locale.translate("error.collection_time_span_years"));
                    return false;
                }
                true
            }
            Syncer::FileSystem => {
                if self.fs_path.is_empty() {
                    page.add_error(locale.translate("error.collection_path"));
                    return false;
                }

                if !Path::new(&self.fs_path).is_dir() {
                    page.add_error(locale.translate("error.collection_existing_dir"));
                    return false;
                }

                true
            }
        }
    }

    pub async fn to_syncer(&self, cur: Option<&SyncerType>) -> anyhow::Result<SyncerType> {
        let syncer = self.syncer.ok_or_else(|| anyhow!("Syncer missing!?"))?;
        let ty = match syncer {
            Syncer::VDirSyncer => {
                let username = match &self.vdir_username {
                    user if !user.is_empty() => Some(user.clone()),
                    _ => None,
                };
                SyncerType::VDirSyncer {
                    email: EmailAccount::new(self.vdir_name.clone(), self.vdir_email.clone()),
                    url: self.vdir_url.clone(),
                    read_only: self.vdir_readonly.is_some(),
                    password: if username.is_some() {
                        Self::determine_password(self.vdir_pw.get(), cur).await?
                    } else {
                        None
                    },
                    username,
                    time_span: Self::fields_to_time_span(
                        &self.vdir_time_span,
                        self.vdir_time_span_years,
                    ),
                }
            }
            Syncer::O365 => SyncerType::O365 {
                email: EmailAccount::new(self.o365_name.clone(), self.o365_email.clone()),
                read_only: self.o365_readonly.is_some(),
                password: Self::determine_o365_password(cur).await?,
                time_span: Self::fields_to_time_span(
                    &self.o365_time_span,
                    self.o365_time_span_years,
                ),
            },
            Syncer::FileSystem => SyncerType::FileSystem {
                path: self.fs_path.clone(),
            },
        };
        Ok(ty)
    }

    async fn determine_password(
        pw: &String,
        cur: Option<&SyncerType>,
    ) -> anyhow::Result<Option<EncryptedPassword>> {
        match pw {
            // we verify before that is PW is not empty on adds (where `cur` is None)
            pw if pw.is_empty() => Ok(cur.unwrap().password().cloned()),
            pw => {
                let secret = retrieve_portal_secret().await?;
                Ok(Some(
                    encrypt_password(&secret, pw).context("Encrypt password")?,
                ))
            }
        }
    }

    async fn determine_o365_password(
        cur: Option<&SyncerType>,
    ) -> anyhow::Result<EncryptedPassword> {
        if let Some(SyncerType::O365 { password, .. }) = cur {
            return Ok(password.clone());
        }

        let secret = retrieve_portal_secret().await?;
        let generated = Uuid::new_v4().simple().to_string();
        encrypt_password(&secret, &generated).context("Encrypt password")
    }

    /// Deserializes the years spinner value, accepting both plain integers and string-encoded
    /// integers as submitted by HTML form fields.
    fn deserialize_years<'de, D>(deserializer: D) -> Result<u32, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<u32>().map_err(de::Error::custom)
    }

    /// Converts a `SyncTimeSpan` to the `(mode, years)` pair used in the form.
    ///
    /// Returns `("years", n)` when the start bound is `Years(n)`, and `("infinite", 5)` (the
    /// default spinner value) when the start bound is `Infinite`.
    fn time_span_to_fields(time_span: &SyncTimeSpan) -> (String, u32) {
        match time_span.start {
            SyncTimeBound::Years(n) => ("years".to_string(), n),
            SyncTimeBound::Infinite => ("infinite".to_string(), DEFAULT_TIME_SPAN_YEARS),
        }
    }

    /// Builds a `SyncTimeSpan` from the form's `(mode, years)` pair.
    ///
    /// When `mode` is `"years"`, the start bound is set to `Years(years)`; otherwise both bounds
    /// are `Infinite`. The end bound is always `Infinite`.
    fn fields_to_time_span(mode: &str, years: u32) -> SyncTimeSpan {
        let start = if mode == "years" {
            SyncTimeBound::Years(years)
        } else {
            SyncTimeBound::Infinite
        };
        SyncTimeSpan {
            start,
            end: SyncTimeBound::Infinite,
        }
    }
}

#[derive(Template)]
#[template(path = "comps/syncer.htm")]
pub struct SyncerTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: &'a str,
    id: String,
    value: SyncerRequest,
    only: Option<Syncer>,
    is_edit: bool,
}

impl<'a> SyncerTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: &'a str,
        value: SyncerRequest,
        only: Option<Syncer>,
        is_edit: bool,
    ) -> Self {
        Self {
            name,
            id: name.replace("[", "_").replace("]", "_"),
            value,
            only,
            locale,
            is_edit,
        }
    }

    pub fn syncer(&self) -> String {
        match self.value.syncer {
            Some(f) => format!("{f}"),
            None => String::from("NONE"),
        }
    }

    pub fn vdir_pw(&self) -> PasswordTemplate {
        PasswordTemplate::new(format!("{}[vdir_pw]", self.name))
    }
}
