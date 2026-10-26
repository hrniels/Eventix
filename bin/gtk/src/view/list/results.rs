// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use eventix_locale::Locale;
use eventix_view::list::{Row, Snapshot};
use gtk::{glib, prelude::*};

use crate::view::indicators::EventIndicators;
use crate::view::response_marker;

use super::{
    item::{Entry, ListItem},
    model::ListModel,
    row::FixedRow,
    scroll::ScrollPosition,
};
use crate::view::action_menu::{self as actions, Actions, OccurrenceTarget};

const ICON_SIZE: i32 = 20;
type Bindings = RefCell<HashMap<gtk::ListItem, (ListItem, glib::SignalHandlerId)>>;

/// Renders a virtualized list with presentation data loaded on demand.
pub(super) struct Results {
    pub scroll: gtk::ScrolledWindow,
    pub model: ListModel,
    pub widget: gtk::Stack,
    pub actions: Rc<Actions<Row>>,
    pub occurrence_actions: Rc<Actions<OccurrenceTarget>>,
    empty: gtk::Label,
    locale: Rc<RefCell<Arc<dyn Locale + Send + Sync>>>,
    scroll_position: Rc<ScrollPosition>,
}

impl Results {
    pub fn new() -> Self {
        let model: ListModel = glib::Object::new();
        let locale = Rc::new(RefCell::new(eventix_locale::default()));
        let actions = Rc::new(Actions::default());
        let occurrence_actions = Rc::new(Actions::default());
        let builder = gtk::Builder::from_resource(resource_path!("view/list/results.ui"));
        let list: gtk::ListView = builder.object("list").unwrap();
        let scroll: gtk::ScrolledWindow = builder.object("scroll").unwrap();
        let scroll_position = ScrollPosition::new(&scroll, &list);
        let factory = factory(
            model.clone(),
            locale.clone(),
            actions.clone(),
            occurrence_actions.clone(),
            scroll_position.clone(),
        );
        let selection = gtk::NoSelection::new(Some(model.clone()));
        list.set_model(Some(&selection));
        list.set_factory(Some(&factory));
        Self {
            scroll,
            model,
            widget: builder.object("results").unwrap(),
            actions,
            occurrence_actions,
            empty: builder.object("empty").unwrap(),
            locale,
            scroll_position,
        }
    }

    pub fn reset_scroll(&self) {
        self.scroll_position.reset();
    }

    pub fn update(&self, snapshot: Snapshot) {
        self.empty
            .set_text(snapshot.locale.translate("No items found"));
        self.locale.replace(snapshot.locale);
        let empty = snapshot.items.is_empty();
        self.model.set_items(snapshot.items);
        if empty {
            self.widget.set_visible_child(&self.empty);
        } else {
            self.widget.set_visible_child(&self.scroll);
        }
    }
}

fn factory(
    model: ListModel,
    locale: Rc<RefCell<Arc<dyn Locale + Send + Sync>>>,
    actions: Rc<Actions<Row>>,
    occurrence_actions: Rc<Actions<OccurrenceTarget>>,
    scroll_position: Rc<ScrollPosition>,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let bindings = Rc::new(Bindings::default());
    let weak = Rc::downgrade(&bindings);
    let position = scroll_position.clone();
    model.connect_changing(move |start, removed| {
        position.hold();
        if let Some(bindings) = weak.upgrade() {
            move_focus_before_change(&bindings, start, removed);
        }
    });
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        item.set_activatable(false);
        item.set_selectable(false);
        item.set_child(Some(&FixedRow::new()));
    });
    let bound = bindings.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let data = item.item().and_downcast::<ListItem>().unwrap();
        let row = item.child().and_downcast::<FixedRow>().unwrap();
        update_row(
            &row,
            &data,
            &**locale.borrow(),
            &actions,
            &occurrence_actions,
            &model,
        );
        let weak = row.downgrade();
        let locale = locale.clone();
        let actions = actions.clone();
        let occurrence_actions = occurrence_actions.clone();
        let model = model.clone();
        let scroll_position = scroll_position.clone();
        let handler = data.connect_notify_local(Some("loaded"), move |data, _| {
            if let Some(row) = weak.upgrade() {
                scroll_position.hold();
                update_row(
                    &row,
                    data,
                    &**locale.borrow(),
                    &actions,
                    &occurrence_actions,
                    &model,
                );
            }
        });
        bound.borrow_mut().insert(item.clone(), (data, handler));
    });
    factory.connect_unbind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some((data, handler)) = bindings.borrow_mut().remove(item) {
            data.disconnect(handler);
        }
        if let Some(row) = item.child().and_downcast::<FixedRow>() {
            row.set_child(&gtk::Box::new(gtk::Orientation::Horizontal, 0));
        }
    });
    factory
}

fn move_focus_before_change(bindings: &Bindings, start: u32, removed: u32) {
    let bindings = bindings.borrow();
    let Some(focus) = bindings
        .keys()
        .find_map(|item| item.child()?.root()?.focus())
    else {
        return;
    };
    let Some(source) = bindings.keys().find(|item| {
        item.child()
            .is_some_and(|row| focus == row || focus.is_ancestor(&row))
    }) else {
        return;
    };
    let end = start + removed;
    if (removed == 0 && source.position() < start)
        || (removed != 0 && !(start..end).contains(&source.position()))
    {
        return;
    }
    // Keep focus on a surviving, stationary row: removing a focused control sends
    // focus to the first item, while inserting before it makes GTK scroll after it.
    let target = bindings
        .keys()
        .filter(|item| {
            if removed == 0 {
                item.position() < start
            } else {
                !(start..end).contains(&item.position())
            }
        })
        .filter_map(|item| {
            item.child()
                .filter(|row| row.is_mapped())
                .map(|row| (item.position(), row))
        })
        .min_by_key(|(position, _)| position.abs_diff(source.position()));
    drop(bindings);
    if let Some((_, row)) = target {
        row.set_focusable(true);
        row.grab_focus();
    } else if let Some(root) = focus.root() {
        root.set_focus(None::<&gtk::Widget>);
    }
}

fn update_row(
    widget: &FixedRow,
    item: &ListItem,
    locale: &dyn Locale,
    actions: &Rc<Actions<Row>>,
    occurrence_actions: &Rc<Actions<OccurrenceTarget>>,
    model: &ListModel,
) {
    match item.entry().as_ref() {
        Some(Entry::Occurrence(row)) => {
            widget.set_child(&occurrence_content(row, locale, occurrence_actions));
            return;
        }
        Some(Entry::Empty(_)) => {
            let label = gtk::Label::new(Some(locale.translate("No occurrences found")));
            widget.set_child(&label);
            return;
        }
        Some(Entry::More(id, direction, loading)) => {
            let label = if *loading {
                "Loading occurrences…"
            } else if *direction == eventix_view::list::occurrences::Direction::Earlier {
                "Load earlier"
            } else {
                "Load later"
            };
            if let Some(button) = widget.first_child().and_downcast::<gtk::Button>() {
                button.set_label(locale.translate(label));
                button.update_state(&[gtk::accessible::State::Busy(*loading)]);
                return;
            }
            let button = gtk::Button::with_label(locale.translate(label));
            button.add_css_class("flat");
            // Keep the focused control alive and focusable while loading. Replacing or
            // disabling it makes GtkListView focus its first row and scroll to the top.
            button.update_state(&[gtk::accessible::State::Busy(*loading)]);
            let weak = model.downgrade();
            let id = id.clone();
            let direction = *direction;
            button.connect_clicked(move |_| {
                if let Some(model) = weak.upgrade() {
                    model.load_more(&id, direction);
                }
            });
            widget.set_child(&button);
            return;
        }
        _ => {}
    }
    if let Some(row) = item.row().as_ref() {
        let content = row_content(row, locale, actions, model);
        widget.set_child(&content);
    } else {
        widget.set_child(&gtk::Box::new(gtk::Orientation::Horizontal, 0));
    }
}

fn install_expansion(
    content: &gtk::Box,
    summary: &gtk::Label,
    row: &Row,
    locale: &dyn Locale,
    model: &ListModel,
) {
    let id = eventix_state::actions::items::ItemId {
        calendar: row.calendar.clone(),
        uid: row.uid.clone(),
        recurrence: None,
    };
    let expanded = model.is_expanded(&id);
    let icon = gtk::Image::from_icon_name(if expanded {
        "pan-down-symbolic"
    } else {
        "pan-end-symbolic"
    });
    icon.set_pixel_size(16);
    icon.add_css_class("ev_list_disclosure");
    content.set_cursor_from_name(Some("pointer"));
    content.set_focusable(true);
    content.set_tooltip_text(Some(locale.translate(if expanded {
        "Collapse occurrences"
    } else {
        "Expand occurrences"
    })));
    content.update_state(&[gtk::accessible::State::Expanded(Some(expanded))]);
    // Keep the chevron immediately after the summary text, rather than at the far end
    // of the expanding summary column.
    let summary_column = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    summary_column.set_hexpand(true);
    summary_column.set_overflow(gtk::Overflow::Hidden);
    summary.set_hexpand(false);
    content.insert_child_after(&summary_column, summary.prev_sibling().as_ref());
    content.remove(summary);
    summary_column.append(summary);
    summary_column.append(&icon);
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    let weak_content = content.downgrade();
    let weak = model.downgrade();
    let timezone = *locale.timezone();
    let clicked_id = id.clone();
    click.connect_released(move |gesture, _, x, y| {
        let Some(content) = weak_content.upgrade() else {
            return;
        };
        if let Some(menu) = content.last_child()
            && let Some(target) = content.pick(x, y, gtk::PickFlags::DEFAULT)
            && (target == menu || target.is_ancestor(&menu))
        {
            return;
        }
        if let Some(model) = weak.upgrade() {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            model.toggle(&clicked_id, &timezone);
        }
    });
    content.add_controller(click);
    let keys = gtk::EventControllerKey::new();
    let weak_content = content.downgrade();
    let weak = model.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if weak_content
            .upgrade()
            .is_some_and(|content| content.has_focus())
            && modifiers.is_empty()
            && matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::space)
            && let Some(model) = weak.upgrade()
        {
            model.toggle(&id, &timezone);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    content.add_controller(keys);
}

fn occurrence_content(
    row: &eventix_view::list::occurrences::Row,
    locale: &dyn Locale,
    handlers: &Rc<Actions<OccurrenceTarget>>,
) -> gtk::Box {
    EventIndicators::ensure_type();
    let builder = gtk::Builder::from_resource(resource_path!("view/list/row.ui"));
    let content: gtk::Box = builder.object("content").unwrap();
    // The base row's type icon occupies 20px plus 12px spacing before its marker.
    content.set_margin_start(ICON_SIZE + content.spacing());
    content.add_css_class("ev_list_occurrence");
    let kind: gtk::Image = builder.object("kind").unwrap();
    content.insert_child_after(
        &response_marker::new(
            &row.background,
            &row.calendar_name,
            row.response,
            ICON_SIZE,
            locale,
        ),
        Some(&kind),
    );
    content.remove(&kind);
    let summary: gtk::Label = builder.object("summary").unwrap();
    summary.set_text(&row.summary);
    summary.set_tooltip_text(Some(&row.summary));
    if row.completed || row.excluded || row.cancelled {
        summary.add_css_class("ev_list_completed");
    }
    if row.excluded || row.cancelled {
        summary.set_tooltip_text(Some(locale.translate(if row.excluded {
            "Excluded"
        } else {
            "Cancelled"
        })));
    }
    let completed: gtk::Image = builder.object("completed").unwrap();
    completed.set_visible(row.completed);
    completed.set_tooltip_text(Some(locale.translate("Completed")));
    let indicators: EventIndicators = builder.object("indicators").unwrap();
    indicators.set_pixel_size(ICON_SIZE);
    indicators.set_state(false, row.overwritten, row.alarm, locale);
    let date: gtk::Label = builder.object("date_range").unwrap();
    date.set_text(&row.details.date);
    date.set_tooltip_text(Some(&row.details.date));
    let recurrence: gtk::Label = builder.object("recurrence").unwrap();
    recurrence.set_visible(false);
    content.append(&actions::occurrence_button(row, locale, &content, handlers));
    content
}

fn row_content(
    row: &Row,
    locale: &dyn Locale,
    handlers: &Rc<Actions<Row>>,
    model: &ListModel,
) -> gtk::Box {
    EventIndicators::ensure_type();
    let builder = gtk::Builder::from_resource(resource_path!("view/list/row.ui"));
    let content: gtk::Box = builder.object("content").unwrap();
    let kind: gtk::Image = builder.object("kind").unwrap();
    kind.set_icon_name(Some(if row.task {
        "view-task-symbolic"
    } else {
        "x-office-calendar-symbolic"
    }));
    kind.set_tooltip_text(Some(locale.translate(if row.task {
        "Task"
    } else {
        "Event"
    })));
    content.insert_child_after(
        &response_marker::new(
            &row.background,
            &row.calendar_name,
            row.response.map(Into::into),
            ICON_SIZE,
            locale,
        ),
        Some(&kind),
    );
    let summary: gtk::Label = builder.object("summary").unwrap();
    summary.set_text(&row.summary);
    summary.set_tooltip_text(Some(&row.summary));
    if row.completed {
        summary.add_css_class("ev_list_completed");
    }
    let completed: gtk::Image = builder.object("completed").unwrap();
    completed.set_visible(row.completed);
    completed.set_tooltip_text(Some(locale.translate("Completed")));
    let indicators: EventIndicators = builder.object("indicators").unwrap();
    indicators.set_pixel_size(ICON_SIZE);
    indicators.set_state(row.recurrent, false, row.alarm, locale);
    for (id, text) in [
        ("date_range", &row.date_range),
        ("recurrence", &row.recurrence),
    ] {
        let label: gtk::Label = builder.object(id).unwrap();
        label.set_text(text);
        label.set_tooltip_text(Some(text));
        label.set_visible(!text.is_empty());
    }
    let actions = actions::button(row, &row.actions, locale, &content, handlers);
    content.append(&actions);
    if row.recurrent {
        install_expansion(&content, &summary, row, locale, model);
    }
    content
}
