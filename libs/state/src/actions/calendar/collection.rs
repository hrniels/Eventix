// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::anyhow;
use eventix_locale::Locale;

use crate::{
    CollectionSettings, EncryptedPassword, EventixState, State, SyncerType, crypto,
    decrypt_password, encrypt_password, generate_local_password, retrieve_portal_secret,
};

use super::super::Action;

async fn encrypt_o365_token(password: &EncryptedPassword, token: &str) -> anyhow::Result<String> {
    let secret = retrieve_portal_secret().await?;
    let password = decrypt_password(&secret, password)?;
    Ok(crypto::encrypt_davmail_token(&password, token).await?)
}

/// Encrypts and persists a Microsoft refresh token for an O365 collection.
pub struct StoreO365RefreshToken {
    pub col_id: String,
    pub token: String,
}

impl std::fmt::Debug for StoreO365RefreshToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreO365RefreshToken")
            .field("col_id", &self.col_id)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl Action<()> for StoreO365RefreshToken {
    fn description(&self) -> &'static str {
        "Authentication completed."
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<()> {
        let col_id = &self.col_id;
        let token = self.token.as_str();

        let password = {
            let state = state.lock().await;
            let collection = state
                .settings()
                .collections()
                .get(col_id)
                .ok_or_else(|| anyhow!("No collection with id {}", col_id))?;
            match collection.syncer() {
                SyncerType::O365 { password, .. } => password.clone(),
                _ => return Err(anyhow!("Collection '{}' is not an O365 collection", col_id)),
            }
        };

        let encrypted = encrypt_o365_token(&password, token).await?;
        let mut state = state.lock().await;
        let password_matches = matches!(
            state
                .settings()
                .collections()
                .get(col_id)
                .map(|collection| collection.syncer()),
            Some(SyncerType::O365 { password: current, .. }) if current == &password
        );
        if !password_matches {
            return Err(anyhow!(
                "Collection '{}' changed while encrypting its refresh token",
                col_id
            ));
        }
        state.misc_mut().set_collection_token(col_id, encrypted);
        state.misc().write_to_file()?;
        Ok(())
    }
}

/// Creates a collection, persists its settings and refreshes the local store.
///
/// CalDAV collections with a username require a nonempty password, which is encrypted.
/// O365 collections receive a generated encrypted password. Returns an error for invalid or
/// duplicate IDs, credential failures, persistence failures or store refresh failures.
/// Failed persistence removes the new collection from in-memory settings.
pub struct CreateCollection {
    pub id: String,
    pub syncer: SyncerType,
    pub password: Option<String>,
}

impl std::fmt::Debug for CreateCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateCollection")
            .field("id", &self.id)
            .field("syncer", &self.syncer)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl Action<()> for CreateCollection {
    fn description(&self) -> &'static str {
        "Collection added"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<()> {
        let Self {
            id,
            mut syncer,
            password,
        } = *self;
        let id = id.as_str();
        let password = password.as_deref();

        let locale = {
            let state = state.lock().await;
            validate_new_collection(&state, id)?;
            state.locale()
        };

        // The secret service may require user interaction, so resolve credentials without the lock.
        initialize_collection_credentials(&mut syncer, password, locale.as_ref()).await?;

        let mut state = state.lock().await;
        validate_new_collection(&state, id)?;
        state
            .settings_mut()
            .collections_mut()
            .insert(id.into(), CollectionSettings::new(syncer));
        if let Err(error) = state.settings().write_to_file() {
            state.settings_mut().collections_mut().remove(id);
            return Err(error);
        }
        State::refresh_store(&mut state).await
    }
}

fn validate_new_collection(state: &State, id: &str) -> anyhow::Result<()> {
    let locale = state.locale();
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        anyhow::bail!(locale.translate("error.collection_name_chars").to_owned());
    }
    if state.settings().collections().contains_key(id) {
        anyhow::bail!(locale.translate("error.collection_exists").to_owned());
    }
    Ok(())
}

async fn initialize_collection_credentials(
    syncer: &mut SyncerType,
    password: Option<&str>,
    locale: &(dyn Locale + Send + Sync),
) -> anyhow::Result<()> {
    match syncer {
        SyncerType::VDirSyncer {
            username: Some(_),
            password: saved,
            ..
        } => {
            let value = password
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!(locale.translate("error.collection_password").to_owned()))?;
            *saved = Some(encrypt_password(&retrieve_portal_secret().await?, value)?);
        }
        SyncerType::O365 { password, .. } => {
            *password =
                encrypt_password(&retrieve_portal_secret().await?, &generate_local_password())?;
        }
        _ => {}
    }
    Ok(())
}

/// Saves a collection's synchronizer settings and refreshes the local store.
///
/// A missing password preserves current credentials; a supplied CalDAV password is encrypted.
/// Returns an error if the collection is missing, its backend changes, encryption fails,
/// persistence fails, or the store cannot be refreshed. Failed persistence restores the previous
/// in-memory settings.
pub struct SaveCollection {
    pub col_id: String,
    pub syncer: SyncerType,
    pub password: Option<String>,
}

impl std::fmt::Debug for SaveCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveCollection")
            .field("col_id", &self.col_id)
            .field("syncer", &self.syncer)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl Action<()> for SaveCollection {
    fn description(&self) -> &'static str {
        "Collection settings updated"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<()> {
        let Self {
            col_id,
            mut syncer,
            password,
        } = *self;
        let col_id = &col_id;
        let password = password.as_deref();

        if let (
            Some(value),
            SyncerType::VDirSyncer {
                username: Some(_),
                password,
                ..
            },
        ) = (password, &mut syncer)
        {
            let secret = retrieve_portal_secret().await?;
            *password = Some(encrypt_password(&secret, value)?);
        }

        let mut state = state.lock().await;
        let locale = state.locale();
        let collection = state
            .settings_mut()
            .collections_mut()
            .get_mut(col_id)
            .ok_or_else(|| {
                anyhow!(
                    locale
                        .translate("Collection is no longer available")
                        .to_owned()
                )
            })?;

        if std::mem::discriminant(collection.syncer()) != std::mem::discriminant(&syncer) {
            anyhow::bail!(locale.translate("error.syncer_change").to_owned());
        }

        // Empty password fields retain the current credentials, not a stale draft's copy.
        match &mut syncer {
            SyncerType::VDirSyncer {
                username,
                password: saved,
                ..
            } => {
                if username.is_none() {
                    *saved = None;
                } else if password.is_none() {
                    *saved = collection.syncer().password().cloned();
                }
            }
            SyncerType::O365 { password, .. } => {
                if let Some(current) = collection.syncer().password() {
                    *password = current.clone();
                }
            }
            _ => {}
        }

        let previous = collection.syncer().clone();
        collection.set_syncer(syncer);

        if let Err(error) = state.settings().write_to_file() {
            state
                .settings_mut()
                .collections_mut()
                .get_mut(col_id)
                .expect("collection remains present while state is locked")
                .set_syncer(previous);
            return Err(error);
        }

        State::refresh_store(&mut state).await
    }
}
