// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Calendar and collection management, authentication, and synchronization actions.

mod calendars;
mod collection;
mod sync;

pub use calendars::{CreateCalendar, DeleteCalendar, DeleteCalendarByFolder, DeleteCollection};
pub use collection::{CreateCollection, SaveCollection, StoreO365RefreshToken};
pub use sync::{DiscoverCollection, ReloadCalendar, ReloadCollection, SyncAll, SyncCollection};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{State, SyncCancellation, SyncResult, SyncerType, actions::Action};

    use super::{CreateCalendar, CreateCollection, DeleteCalendar, DeleteCollection, SyncAll};

    #[tokio::test]
    async fn calendar_actions_execute_through_the_shared_async_interface() {
        let dir = tempfile::tempdir().unwrap();
        let xdg = Arc::new(crate::with_test_xdg(dir.path(), dir.path()));
        let state = Arc::new(tokio::sync::Mutex::new(State::new(xdg).unwrap()));
        let create: Box<dyn Action<()>> = Box::new(CreateCollection {
            id: "local".into(),
            syncer: SyncerType::FileSystem {
                path: dir.path().join("calendars").display().to_string(),
            },
            password: None,
        });
        assert!(create.affected_items().is_empty());
        create.execute(&state).await.unwrap();
        Box::new(CreateCalendar {
            id: "local".into(),
            name: "Team".into(),
        })
        .execute(&state)
        .await
        .unwrap();
        let cal_id = state.lock().await.settings().collections()["local"]
            .all_calendars()
            .keys()
            .next()
            .unwrap()
            .clone();

        let sync: Box<dyn Action<SyncResult>> = Box::new(SyncAll {
            auth_url: None,
            progress: None,
            cancellation: SyncCancellation::default(),
        });
        assert!(!sync.execute(&state).await.unwrap().cancelled);
        Box::new(DeleteCalendar {
            col_id: "local".into(),
            cal_id,
        })
        .execute(&state)
        .await
        .unwrap();
        assert!(
            state.lock().await.settings().collections()["local"]
                .all_calendars()
                .is_empty()
        );
        Box::new(DeleteCollection {
            col_id: "local".into(),
        })
        .execute(&state)
        .await
        .unwrap();
        assert!(state.lock().await.settings().collections().is_empty());
    }

    #[test]
    fn credential_actions_do_not_expose_plaintext_in_debug_output() {
        let action = CreateCollection {
            id: "local".into(),
            syncer: SyncerType::FileSystem {
                path: "/unused".into(),
            },
            password: Some("sensitive-password".into()),
        };
        assert!(!format!("{action:?}").contains("sensitive-password"));
        let token = super::StoreO365RefreshToken {
            col_id: "remote".into(),
            token: "sensitive-token".into(),
        };
        assert!(!format!("{token:?}").contains("sensitive-token"));
    }
}
