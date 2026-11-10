// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
};

use eventix_locale::Locale;
use eventix_state::actions::items::ItemId;
use gtk::{glib, prelude::*};

use crate::model::list::Row;
use crate::model::list_occurrences::{Direction, Row as OccurrenceRow};
use crate::view::{
    action_menu::{self as actions, Actions, OccurrenceTarget},
    indicators::EventIndicators,
    response_marker::Marker,
};

use super::{
    item::{Entry, ListItem},
    model::ListModel,
    row::FixedRow,
};

const ICON_SIZE: i32 = 20;
type ExpansionTarget = Rc<RefCell<Option<(ItemId, chrono_tz::Tz)>>>;

/// Retains a factory slot's widgets while its bound item changes.
pub(super) struct Content {
    pub widget: FixedRow,
    content: gtk::Box,
    kind: gtk::Image,
    marker: Marker,
    summary: gtk::Label,
    disclosure: gtk::Image,
    completed: gtk::Image,
    indicators: EventIndicators,
    date: gtk::Label,
    recurrence: gtk::Label,
    menu: gtk::MenuButton,
    more: OnceCell<gtk::Button>,
    empty: OnceCell<gtk::Label>,
    expansion: ExpansionTarget,
    pagination: Rc<RefCell<Option<(ItemId, Direction)>>>,
    model: glib::WeakRef<ListModel>,
}

impl Content {
    pub fn new(model: &ListModel) -> Self {
        EventIndicators::ensure_type();
        let builder = gtk::Builder::from_resource(resource_path!("view/list/row.ui"));
        let content: gtk::Box = builder.object("content").unwrap();
        let kind: gtk::Image = builder.object("kind").unwrap();
        let marker = Marker::new(ICON_SIZE);
        content.insert_child_after(&marker.widget, Some(&kind));
        let summary: gtk::Label = builder.object("summary").unwrap();
        let summary_column = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        summary_column.set_hexpand(true);
        summary_column.set_overflow(gtk::Overflow::Hidden);
        summary.set_hexpand(false);
        content.insert_child_after(&summary_column, summary.prev_sibling().as_ref());
        content.remove(&summary);
        summary_column.append(&summary);
        let disclosure = gtk::Image::from_icon_name("pan-end-symbolic");
        disclosure.set_pixel_size(16);
        disclosure.add_css_class("ev_list_disclosure");
        summary_column.append(&disclosure);
        let indicators: EventIndicators = builder.object("indicators").unwrap();
        indicators.set_pixel_size(ICON_SIZE);
        let menu = actions::empty_button(&content);
        content.append(&menu);
        let expansion = ExpansionTarget::default();
        install_expansion(&content, &expansion, model);
        let widget = FixedRow::new();
        widget.set_child(&content);
        content.set_visible(false);
        Self {
            widget,
            content,
            kind,
            marker,
            summary,
            disclosure,
            indicators,
            menu,
            completed: builder.object("completed").unwrap(),
            date: builder.object("date_range").unwrap(),
            recurrence: builder.object("recurrence").unwrap(),
            more: OnceCell::new(),
            empty: OnceCell::new(),
            expansion,
            pagination: Rc::default(),
            model: model.downgrade(),
        }
    }

    pub fn update(
        &self,
        item: &ListItem,
        locale: &dyn Locale,
        handlers: &Rc<Actions<Row>>,
        occurrence_handlers: &Rc<Actions<OccurrenceTarget>>,
    ) {
        match item.entry().as_ref() {
            Some(Entry::Occurrence(row)) => {
                self.update_occurrence(row, locale, occurrence_handlers);
            }
            Some(Entry::Empty(_)) => {
                self.clear();
                let label = self.empty.get_or_init(|| gtk::Label::new(None));
                label.set_text(locale.translate("No occurrences found"));
                self.show(label);
            }
            Some(Entry::More(id, direction, loading)) => {
                self.expansion.take();
                self.pagination.replace(Some((id.clone(), *direction)));
                let button = self.more.get_or_init(|| self.pagination_button());
                let label = if *loading {
                    "Loading occurrences…"
                } else if *direction == Direction::Earlier {
                    "Load earlier"
                } else {
                    "Load later"
                };
                button.set_label(locale.translate(label));
                // Keep this control alive and focusable while a page is loaded.
                button.update_state(&[gtk::accessible::State::Busy(*loading)]);
                self.show(button);
            }
            _ => {
                if let Some(row) = item.row().as_ref() {
                    self.update_base(row, locale, handlers);
                } else {
                    self.clear();
                }
            }
        }
    }

    fn update_base(&self, row: &Row, locale: &dyn Locale, handlers: &Rc<Actions<Row>>) {
        self.set_occurrence(false);
        self.kind.set_icon_name(Some(if row.task {
            "view-task-symbolic"
        } else {
            "x-office-calendar-symbolic"
        }));
        self.kind
            .set_tooltip_text(Some(locale.translate(if row.task {
                "Task"
            } else {
                "Event"
            })));
        self.marker.update(
            &row.background,
            &row.calendar_name,
            row.response.map(Into::into),
            locale,
        );
        self.set_summary(&row.summary, row.completed);
        self.set_completed(row.completed, locale);
        self.indicators
            .set_state(row.recurrent, false, row.alarm, locale);
        set_text(&self.date, &row.date_range);
        set_text(&self.recurrence, &row.recurrence);
        self.set_expansion(
            row.recurrent.then(|| ItemId {
                calendar: row.calendar.clone(),
                uid: row.uid.clone(),
                recurrence: None,
            }),
            locale,
        );
        actions::update_button(&self.menu, row, &row.actions, locale, handlers);
        self.show(&self.content);
    }

    fn update_occurrence(
        &self,
        row: &OccurrenceRow,
        locale: &dyn Locale,
        handlers: &Rc<Actions<OccurrenceTarget>>,
    ) {
        self.set_occurrence(true);
        self.marker
            .update(&row.background, &row.calendar_name, row.response, locale);
        self.set_summary(&row.summary, row.completed || row.excluded || row.cancelled);
        if row.excluded || row.cancelled {
            self.summary
                .set_tooltip_text(Some(locale.translate(if row.excluded {
                    "Excluded"
                } else {
                    "Cancelled"
                })));
        }
        self.set_completed(row.completed, locale);
        self.indicators
            .set_state(false, row.overwritten, row.alarm, locale);
        set_text(&self.date, &row.details.date);
        set_text(&self.recurrence, "");
        self.set_expansion(None, locale);
        actions::update_occurrence_button(&self.menu, row, locale, handlers);
        self.show(&self.content);
    }

    /// Drops item-specific callbacks and targets without discarding the reusable widgets.
    pub fn clear(&self) {
        self.expansion.take();
        self.pagination.take();
        actions::clear_button(&self.menu);
        if let Some(child) = self.widget.first_child() {
            child.set_visible(false);
        }
    }

    pub fn open_menu(&self) {
        if self.menu.is_mapped() {
            self.menu.popup();
        } else if let Some(button) = self.more.get().filter(|button| button.is_mapped()) {
            button.emit_clicked();
        }
    }

    pub fn toggle_occurrences(&self, expanded: Option<bool>) {
        let target = self.expansion.borrow().clone();
        if let Some((id, timezone)) = target
            && let Some(model) = self.model.upgrade()
            && expanded.is_none_or(|expanded| model.is_expanded(&id) != expanded)
        {
            model.toggle(&id, &timezone);
        }
    }

    fn show(&self, child: &impl IsA<gtk::Widget>) {
        self.widget.set_child(child);
        child.set_visible(true);
    }

    fn set_occurrence(&self, occurrence: bool) {
        if occurrence {
            if self.kind.parent().is_some() {
                self.content.remove(&self.kind);
            }
            self.content
                .set_margin_start(ICON_SIZE + self.content.spacing());
            self.content.add_css_class("ev_list_occurrence");
        } else {
            if self.kind.parent().is_none() {
                self.content.prepend(&self.kind);
            }
            self.content.set_margin_start(0);
            self.content.remove_css_class("ev_list_occurrence");
        }
    }

    fn set_summary(&self, summary: &str, completed: bool) {
        self.summary.set_text(summary);
        self.summary.set_tooltip_text(Some(summary));
        if completed {
            self.summary.add_css_class("ev_list_completed");
        } else {
            self.summary.remove_css_class("ev_list_completed");
        }
    }

    fn set_completed(&self, completed: bool, locale: &dyn Locale) {
        self.completed.set_visible(completed);
        self.completed
            .set_tooltip_text(Some(locale.translate("Completed")));
    }

    fn set_expansion(&self, id: Option<ItemId>, locale: &dyn Locale) {
        let expanded = id.as_ref().map(|id| {
            self.model
                .upgrade()
                .is_some_and(|model| model.is_expanded(id))
        });
        self.disclosure.set_visible(id.is_some());
        self.disclosure
            .set_icon_name(Some(if expanded == Some(true) {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            }));
        self.content.set_focusable(false);
        self.content
            .set_cursor_from_name(id.as_ref().map(|_| "pointer"));
        self.content.set_tooltip_text(id.as_ref().map(|_| {
            locale.translate(if expanded == Some(true) {
                "Collapse occurrences"
            } else {
                "Expand occurrences"
            })
        }));
        self.content
            .update_state(&[gtk::accessible::State::Expanded(expanded)]);
        self.expansion
            .replace(id.map(|id| (id, *locale.timezone())));
    }

    fn pagination_button(&self) -> gtk::Button {
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.add_css_class("ev_list_pagination");
        let target = self.pagination.clone();
        let model = self.model.clone();
        button.connect_clicked(move |_| {
            let target = target.borrow().clone();
            if let Some((id, direction)) = target
                && let Some(model) = model.upgrade()
            {
                model.load_more(&id, direction);
            }
        });
        button
    }
}

fn set_text(label: &gtk::Label, text: &str) {
    label.set_text(text);
    label.set_tooltip_text(Some(text));
    label.set_visible(!text.is_empty());
}

fn install_expansion(content: &gtk::Box, target: &ExpansionTarget, model: &ListModel) {
    // Controllers outlive bindings. Copy the current target before dispatch, since
    // expanding a series can synchronously refresh this slot and replace its target.
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    let weak_content = content.downgrade();
    let weak_model = model.downgrade();
    let clicked = target.clone();
    click.connect_released(move |gesture, _, x, y| {
        let Some(content) = weak_content.upgrade() else {
            return;
        };
        if let Some(menu) = content.last_child()
            && let Some(widget) = content.pick(x, y, gtk::PickFlags::DEFAULT)
            && (widget == menu || widget.is_ancestor(&menu))
        {
            return;
        }
        let target = clicked.borrow().clone();
        if let Some((id, timezone)) = target
            && let Some(model) = weak_model.upgrade()
        {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            model.toggle(&id, &timezone);
        }
    });
    content.add_controller(click);
}
