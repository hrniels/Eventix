// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::{Duration, Instant},
};

use chrono::Utc;
use eventix_ical::objects::{CalDate, CalDateType, DateContext};
use eventix_state::actions::items::ItemId;
use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use crate::model::list::Row;
use crate::model::list_occurrences::{Cursor, Direction};
use crate::worker::event::{ListRowsResponse, OccurrencesResponse};
use crate::worker::message::{ListRowsRequest, OccurrencesRequest};

use super::item::{Entry, ListItem};

type Key = (String, String);
type LoadRows = Rc<dyn Fn(ListRowsRequest)>;
type LoadOccurrences = Rc<dyn Fn(OccurrencesRequest)>;
type Truncating = Rc<dyn Fn(u32)>;
const SCROLL_DELAY: Duration = Duration::from_millis(35);
const MAX_SCROLL_DELAY: Duration = Duration::from_millis(75);
const OCCURRENCES_PER_PAGE: usize = 3;

struct CachedRow {
    row: Option<Row>,
    loaded: bool,
    fresh: bool,
}

struct Expansion {
    token: u64,
    anchor: Cursor,
    rows: Vec<std::sync::Arc<crate::model::list_occurrences::Row>>,
    earlier: Option<Cursor>,
    later: Option<Cursor>,
    loading_earlier: bool,
    loading_later: bool,
    earlier_count: usize,
    later_count: usize,
}

fn key(item: &ItemId) -> Key {
    (item.calendar.clone(), item.uid.clone())
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ListModel {
        pub items: RefCell<Vec<ItemId>>,
        pub(super) entries: RefCell<Vec<Entry>>,
        pub(super) expanded: RefCell<BTreeMap<Key, Expansion>>,
        pub token: Cell<u64>,
        pub live: RefCell<BTreeMap<Key, glib::WeakRef<ListItem>>>,
        pub slots: RefCell<BTreeMap<u32, glib::WeakRef<ListItem>>>,
        pub viewport: Cell<(u32, u32)>,
        pub scrolling_since: Cell<Option<Instant>>,
        pub requested: RefCell<BTreeSet<Key>>,
        pub fresh: RefCell<BTreeMap<Key, glib::WeakRef<ListItem>>>,
        pub load_source: RefCell<Option<glib::SourceId>>,
        pub revision: Cell<u64>,
        pub(super) load: RefCell<Option<LoadRows>>,
        pub(super) load_occurrences: RefCell<Option<LoadOccurrences>>,
        pub(super) truncating: RefCell<Option<Truncating>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ListModel {
        const NAME: &'static str = "EventixListModel";
        type Type = super::ListModel;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for ListModel {
        fn dispose(&self) {
            self.obj().cancel_pending();
        }
    }

    impl ListModelImpl for ListModel {
        fn item_type(&self) -> glib::Type {
            ListItem::static_type()
        }

        fn n_items(&self) -> u32 {
            self.entries.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let entry = self.entries.borrow().get(position as usize)?.clone();
            if let Some(item) = self
                .slots
                .borrow()
                .get(&position)
                .and_then(glib::WeakRef::upgrade)
            {
                return Some(item.upcast());
            }
            let item: ListItem = glib::Object::new();
            if let Entry::Base(id) = &entry {
                self.live.borrow_mut().insert(key(id), item.downgrade());
                self.obj().schedule_load(Duration::ZERO, false);
            }
            item.set_entry(entry);
            self.slots.borrow_mut().insert(position, item.downgrade());
            Some(item.upcast())
        }
    }
}

glib::wrapper! {
    pub struct ListModel(ObjectSubclass<imp::ListModel>)
        @implements gio::ListModel;
}

impl ListModel {
    pub(super) fn connect_truncating(&self, truncating: impl Fn(u32) + 'static) {
        self.imp().truncating.replace(Some(Rc::new(truncating)));
    }
    pub(super) fn connect_load_rows(&self, load: impl Fn(ListRowsRequest) + 'static) {
        self.imp().load.replace(Some(Rc::new(load)));
    }

    pub fn set_items(&self, items: Vec<ItemId>) {
        let imp = self.imp();
        imp.revision.set(imp.revision.get() + 1);
        self.cancel_pending();
        let keys = items.iter().map(key).collect::<BTreeSet<_>>();
        imp.items.replace(items);
        imp.expanded
            .borrow_mut()
            .retain(|key, _| keys.contains(key));
        imp.live
            .borrow_mut()
            .retain(|key, item| keys.contains(key) && item.upgrade().is_some());
        self.rebuild();
        self.schedule_load(Duration::ZERO, true);
        let children: Vec<_> = imp
            .slots
            .borrow()
            .values()
            .filter_map(glib::WeakRef::upgrade)
            .filter(|item| !matches!(item.entry().as_ref(), Some(Entry::Base(_))))
            .collect();
        for child in children {
            child.notify("loaded");
        }
        let expanded: Vec<_> = imp.expanded.borrow().keys().cloned().collect();
        for (calendar, uid) in expanded {
            self.refresh_expansion(&ItemId {
                calendar,
                uid,
                recurrence: None,
            });
        }
    }

    pub(super) fn connect_load_occurrences(&self, load: impl Fn(OccurrencesRequest) + 'static) {
        self.imp().load_occurrences.replace(Some(Rc::new(load)));
    }

    pub(super) fn is_expanded(&self, item: &ItemId) -> bool {
        self.imp().expanded.borrow().contains_key(&key(item))
    }

    pub(super) fn toggle(&self, item: &ItemId, timezone: &chrono_tz::Tz) {
        let imp = self.imp();
        if imp.expanded.borrow_mut().remove(&key(item)).is_some() {
            self.rebuild();
            self.notify_base(item);
            return;
        }
        let today = Utc::now().with_timezone(timezone).date_naive();
        let anchor = Cursor {
            start: DateContext::system()
                .date(&CalDate::Date(today, CalDateType::Inclusive))
                .start_in(timezone),
            recurrence: String::new(),
        };
        imp.expanded.borrow_mut().insert(
            key(item),
            Expansion {
                token: 0,
                anchor: anchor.clone(),
                rows: Vec::new(),
                earlier: Some(anchor.clone()),
                later: Some(anchor),
                loading_earlier: false,
                loading_later: false,
                earlier_count: OCCURRENCES_PER_PAGE,
                later_count: OCCURRENCES_PER_PAGE,
            },
        );
        self.refresh_expansion(item);
        self.notify_base(item);
    }

    fn refresh_expansion(&self, item: &ItemId) {
        let imp = self.imp();
        imp.token.set(imp.token.get() + 1);
        let requests = {
            let mut expanded = imp.expanded.borrow_mut();
            let Some(expansion) = expanded.get_mut(&key(item)) else {
                return;
            };
            expansion.token = imp.token.get();
            expansion.loading_earlier = true;
            expansion.loading_later = true;
            expansion.earlier = Some(expansion.anchor.clone());
            expansion.later = Some(expansion.anchor.clone());
            // Keep displayed rows until both sides are replaced, so refresh preserves geometry.
            [Direction::Earlier, Direction::From].map(|direction| OccurrencesRequest {
                revision: imp.revision.get(),
                token: expansion.token,
                item: item.clone(),
                cursor: expansion.anchor.clone(),
                direction,
                count: if direction == Direction::Earlier {
                    expansion.earlier_count
                } else {
                    expansion.later_count
                },
            })
        };
        self.rebuild();
        for request in requests {
            self.request_occurrences(request);
        }
    }

    pub(super) fn load_more(&self, item: &ItemId, direction: Direction) {
        let request = {
            let mut expanded = self.imp().expanded.borrow_mut();
            let Some(expansion) = expanded.get_mut(&key(item)) else {
                return;
            };
            let (cursor, loading) = if direction == Direction::Earlier {
                (&expansion.earlier, &mut expansion.loading_earlier)
            } else {
                (&expansion.later, &mut expansion.loading_later)
            };
            if *loading {
                return;
            }
            let Some(cursor) = cursor.clone() else { return };
            *loading = true;
            let direction = if direction == Direction::Later && cursor == expansion.anchor {
                Direction::From
            } else {
                direction
            };
            OccurrencesRequest {
                revision: self.imp().revision.get(),
                token: expansion.token,
                item: item.clone(),
                cursor,
                direction,
                count: OCCURRENCES_PER_PAGE,
            }
        };
        self.rebuild();
        self.request_occurrences(request);
    }

    fn request_occurrences(&self, request: OccurrencesRequest) {
        let load = self.imp().load_occurrences.borrow().clone();
        if let Some(load) = load {
            load(request);
        }
    }

    pub(super) fn set_occurrences(&self, response: OccurrencesResponse) -> Result<(), String> {
        let request = response.request;
        if request.revision != self.imp().revision.get() {
            return Ok(());
        }
        let result = {
            let mut expanded = self.imp().expanded.borrow_mut();
            let Some(expansion) = expanded.get_mut(&key(&request.item)) else {
                return Ok(());
            };
            if expansion.token != request.token {
                return Ok(());
            }
            let earlier = request.direction == Direction::Earlier;
            if earlier {
                expansion.loading_earlier = false;
            } else {
                expansion.loading_later = false;
            }
            response.result.map(|page| {
                // Initial/refresh pages replace their side of the anchor; pagination appends.
                if request.cursor == expansion.anchor {
                    expansion
                        .rows
                        .retain(|row| (row.cursor < expansion.anchor) != earlier);
                }
                let boundary = if earlier {
                    page.rows.first()
                } else {
                    page.rows.last()
                }
                .map(|row| row.cursor.clone())
                .filter(|_| page.more);
                if earlier {
                    expansion.earlier = boundary;
                } else {
                    expansion.later = boundary;
                }
                expansion.rows.extend(page.rows);
                expansion.rows.sort_by(|a, b| a.cursor.cmp(&b.cursor));
                expansion.rows.dedup_by(|a, b| a.item == b.item);
                expansion.earlier_count = expansion
                    .rows
                    .iter()
                    .filter(|row| row.cursor < expansion.anchor)
                    .count()
                    .max(OCCURRENCES_PER_PAGE);
                expansion.later_count = expansion
                    .rows
                    .iter()
                    .filter(|row| row.cursor >= expansion.anchor)
                    .count()
                    .max(OCCURRENCES_PER_PAGE);
            })
        };
        self.rebuild();
        result
    }

    fn notify_base(&self, item: &ItemId) {
        let live = self
            .imp()
            .live
            .borrow()
            .get(&key(item))
            .and_then(glib::WeakRef::upgrade);
        if let Some(live) = live {
            live.notify("loaded");
        }
    }

    fn rebuild(&self) {
        let imp = self.imp();
        let mut entries = Vec::new();
        for item in imp.items.borrow().iter() {
            entries.push(Entry::Base(item.clone()));
            if let Some(expansion) = imp.expanded.borrow().get(&key(item)) {
                if expansion.earlier.is_some() {
                    entries.push(Entry::More(
                        item.clone(),
                        Direction::Earlier,
                        expansion.loading_earlier,
                    ));
                }
                entries.extend(expansion.rows.iter().cloned().map(Entry::Occurrence));
                if expansion.rows.is_empty()
                    && !expansion.loading_earlier
                    && !expansion.loading_later
                {
                    entries.push(Entry::Empty(item.clone()));
                }
                if expansion.later.is_some() {
                    entries.push(Entry::More(
                        item.clone(),
                        Direction::Later,
                        expansion.loading_later,
                    ));
                }
            }
        }
        self.replace_entries(entries);
    }

    /// Updates fixed-position slots so GTK never loses its viewport anchor to an insertion.
    fn replace_entries(&self, entries: Vec<Entry>) {
        let imp = self.imp();
        let old_len = imp.entries.borrow().len() as u32;
        let new_len = entries.len() as u32;
        let revision = imp.revision.get();
        if new_len < old_len
            && let Some(truncating) = imp.truncating.borrow().clone()
        {
            truncating(new_len);
        }

        // Preserve loaded domain data while the presentation slots change their targets.
        let rows: BTreeMap<_, _> = imp
            .live
            .borrow()
            .iter()
            .filter_map(|(key, item)| {
                let item = item.upgrade()?;
                let fresh = imp.fresh.borrow().get(key).and_then(glib::WeakRef::upgrade);
                Some((
                    key.clone(),
                    CachedRow {
                        row: item.row().clone(),
                        loaded: item.loaded(),
                        fresh: fresh.as_ref() == Some(&item),
                    },
                ))
            })
            .collect();
        imp.entries.replace(entries);
        let updates = self.update_slots(&rows);
        // Only the tail grows or shrinks. Existing GTK row identities stay at their
        // original positions even when occurrences are inserted or a series moves.
        if old_len != new_len {
            self.items_changed(
                old_len.min(new_len),
                old_len.saturating_sub(new_len),
                new_len.saturating_sub(old_len),
            );
        }
        for item in updates {
            if imp.revision.get() != revision {
                return;
            }
            item.notify("loaded");
        }
        self.schedule_load(Duration::ZERO, false);
    }

    fn update_slots(&self, rows: &BTreeMap<Key, CachedRow>) -> Vec<ListItem> {
        let imp = self.imp();
        let mut updates = Vec::new();
        let mut live = BTreeMap::new();
        let mut fresh = BTreeMap::new();
        imp.slots.borrow_mut().retain(|position, weak| {
            let Some(item) = weak.upgrade() else {
                return false;
            };
            let Some(entry) = imp.entries.borrow().get(*position as usize).cloned() else {
                return false;
            };
            let changed = item
                .entry()
                .as_ref()
                .is_none_or(|old| !old.same_content(&entry));
            if let Entry::Base(id) = &entry {
                let key = key(id);
                if rows.get(&key).is_some_and(|row| row.fresh) {
                    fresh.insert(key.clone(), item.downgrade());
                }
                live.insert(key, item.downgrade());
            }
            if changed {
                let row = match &entry {
                    Entry::Base(id) => rows.get(&key(id)),
                    _ => None,
                };
                item.replace_content(
                    entry,
                    row.and_then(|row| row.row.clone()),
                    row.is_some_and(|row| row.loaded),
                );
                updates.push(item);
            }
            true
        });
        imp.live.replace(live);
        imp.fresh.replace(fresh);
        updates
    }

    /// Updates the visible index range, coalescing small scroll movements but not page jumps.
    pub(super) fn set_viewport(&self, first: u32, end: u32, resized: bool) {
        let imp = self.imp();
        let previous = imp.viewport.replace((first, end));
        if previous == (first, end) && !resized {
            return;
        }
        let jumped = first >= previous.1 || end <= previous.0;
        let delay = if resized || jumped {
            imp.scrolling_since.set(None);
            Duration::ZERO
        } else {
            let now = Instant::now();
            let started = imp.scrolling_since.get().unwrap_or(now);
            imp.scrolling_since.set(Some(started));
            SCROLL_DELAY.min(MAX_SCROLL_DELAY.saturating_sub(now.duration_since(started)))
        };
        self.schedule_load(delay, true);
    }

    /// Discards work belonging to the old list and releases its timers.
    fn cancel_pending(&self) {
        let imp = self.imp();
        if let Some(source) = imp.load_source.take() {
            source.remove();
        }
        imp.scrolling_since.set(None);
        imp.requested.borrow_mut().clear();
        imp.fresh.borrow_mut().clear();
    }

    fn schedule_load(&self, delay: Duration, replace: bool) {
        let imp = self.imp();
        if imp.load_source.borrow().is_some() {
            if !replace {
                return;
            }
            if let Some(source) = imp.load_source.take() {
                source.remove();
            }
        }
        let weak = self.downgrade();
        let load = move || {
            if let Some(model) = weak.upgrade() {
                model.imp().load_source.take();
                model.load_pending();
            }
        };
        let source = if delay.is_zero() {
            glib::idle_add_local_once(load)
        } else {
            glib::timeout_add_local_once(delay, load)
        };
        imp.load_source.replace(Some(source));
    }

    fn load_pending(&self) {
        let imp = self.imp();
        imp.scrolling_since.set(None);
        imp.live
            .borrow_mut()
            .retain(|_, item| item.upgrade().is_some());
        imp.slots
            .borrow_mut()
            .retain(|_, item| item.upgrade().is_some());
        imp.fresh
            .borrow_mut()
            .retain(|_, item| item.upgrade().is_some());
        let items = self
            .visible_items()
            .into_iter()
            .filter(|id| {
                let key = key(id);
                if imp.requested.borrow().contains(&key) {
                    return false;
                }
                let item = imp.live.borrow().get(&key).and_then(glib::WeakRef::upgrade);
                let fresh = imp
                    .fresh
                    .borrow()
                    .get(&key)
                    .and_then(glib::WeakRef::upgrade);
                item.is_some() && item != fresh
            })
            .collect::<Vec<_>>();
        if !items.is_empty()
            && let Some(load) = imp.load.borrow().clone()
        {
            imp.requested.borrow_mut().extend(items.iter().map(key));
            let (first, end) = imp.viewport.get();
            tracing::debug!(
                revision = imp.revision.get(),
                first,
                end,
                count = items.len(),
                "Loading visible list rows"
            );
            tracing::trace!(?items, "Requested list row identities");
            load(ListRowsRequest {
                revision: imp.revision.get(),
                items,
            });
        }
    }

    pub(super) fn set_rows(&self, response: ListRowsResponse) -> Result<(), String> {
        let imp = self.imp();
        if response.revision != imp.revision.get() {
            return Ok(());
        }
        let rows = match response.result {
            Ok(rows) => rows,
            Err(error) => {
                imp.requested.borrow_mut().clear();
                return Err(error);
            }
        };
        let mut visible: BTreeSet<_> = self.visible_items().iter().map(key).collect();
        // Only the current page is presented, in one main-loop turn. Results from
        // pages passed during scrolling must not spend time updating cached widgets.
        for (id, row) in rows {
            let item_key = key(&id);
            imp.requested.borrow_mut().remove(&item_key);
            if row.as_ref().is_none_or(|row| !row.recurrent)
                && imp.expanded.borrow_mut().remove(&item_key).is_some()
            {
                self.rebuild();
                visible = self.visible_items().iter().map(key).collect();
            }
            // Rebuilding or notifying a row can synchronously replace the list.
            if imp.revision.get() != response.revision {
                break;
            }
            if !visible.contains(&item_key) {
                continue;
            }
            let item = imp
                .live
                .borrow()
                .get(&item_key)
                .and_then(glib::WeakRef::upgrade);
            if let Some(item) = item {
                imp.fresh.borrow_mut().insert(item_key, item.downgrade());
                item.set_row(row);
            }
            if imp.revision.get() != response.revision {
                break;
            }
        }
        Ok(())
    }

    fn visible_items(&self) -> Vec<ItemId> {
        let (first, end) = self.imp().viewport.get();
        self.imp()
            .entries
            .borrow()
            .iter()
            .skip(first as usize)
            .take(end.saturating_sub(first) as usize)
            .filter_map(|entry| match entry {
                Entry::Base(id) => Some(id.clone()),
                _ => None,
            })
            .collect()
    }
}
