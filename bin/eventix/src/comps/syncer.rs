use askama::Template;
use axum::http::Uri;
use email_address::EmailAddress;
use eventix_locale::Locale;
use eventix_state::{EmailAccount, SyncerType};
use formatx::formatx;
use serde::{Deserialize, Deserializer};
use std::fmt::{self, Display};
use std::path::Path;
use std::sync::Arc;

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

#[derive(Debug, Default, Deserialize)]
pub struct SyncerRequest {
    #[serde(deserialize_with = "Syncer::deserialize")]
    syncer: Option<Syncer>,
    vdir_name: String,
    vdir_email: String,
    vdir_url: String,
    vdir_readonly: Option<String>,
    vdir_username: String,
    vdir_pw_cmd: String,
    o365_name: String,
    o365_email: String,
    o365_readonly: Option<String>,
    o365_pw_cmd: String,
    fs_path: String,
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
                password_cmd,
            } => {
                sync.vdir_name = email.name().clone();
                sync.vdir_email = email.org_address().clone();
                sync.vdir_readonly = match *read_only {
                    true => Some(String::new()),
                    false => None,
                };
                sync.vdir_url = url.clone();
                sync.vdir_username = username.clone().unwrap_or_default();
                sync.vdir_pw_cmd = password_cmd
                    .as_ref()
                    .map(|vec| vec.join(" "))
                    .unwrap_or_default();
            }

            SyncerType::O365 {
                email,
                read_only,
                password_cmd,
            } => {
                sync.o365_name = email.name().clone();
                sync.o365_email = email.org_address().clone();
                sync.o365_readonly = match *read_only {
                    true => Some(String::new()),
                    false => None,
                };
                sync.o365_pw_cmd = password_cmd.join(" ");
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

    pub fn check(&self, page: &mut Page, locale: &Arc<dyn Locale + Send + Sync>) -> bool {
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

    pub fn to_syncer(&self) -> Option<SyncerType> {
        let syncer = self.syncer?;
        let ty = match syncer {
            Syncer::VDirSyncer => SyncerType::VDirSyncer {
                email: EmailAccount::new(self.vdir_name.clone(), self.vdir_email.clone()),
                url: self.vdir_url.clone(),
                read_only: self.vdir_readonly.is_some(),
                username: match &self.vdir_username {
                    user if !user.is_empty() => Some(user.clone()),
                    _ => None,
                },
                password_cmd: Self::make_pw_cmd(&self.vdir_pw_cmd),
            },
            Syncer::O365 => SyncerType::O365 {
                email: EmailAccount::new(self.o365_name.clone(), self.o365_email.clone()),
                read_only: self.o365_readonly.is_some(),
                password_cmd: Self::make_pw_cmd(&self.o365_pw_cmd).unwrap(),
            },
            Syncer::FileSystem => SyncerType::FileSystem {
                path: self.fs_path.clone(),
            },
        };
        Some(ty)
    }

    fn make_pw_cmd(cmd: &str) -> Option<Vec<String>> {
        match cmd
            .split_whitespace()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
        {
            vec if !vec.is_empty() => Some(vec),
            _ => None,
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
}

impl<'a> SyncerTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: &'a str,
        value: SyncerRequest,
        only: Option<Syncer>,
    ) -> Self {
        Self {
            name,
            id: name.replace("[", "_").replace("]", "_"),
            value,
            only,
            locale,
        }
    }

    pub fn syncer(&self) -> String {
        match self.value.syncer {
            Some(f) => format!("{f}"),
            None => String::from("NONE"),
        }
    }
}
