// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use eventix_ical::objects::CalPartStat;
use eventix_locale::Locale;
use eventix_state::actions::items::{
    EditScope, ItemId, OccurrenceAction, TaskAction, UpdatePersonalAlarms,
};
use gtk::{glib, graphene, prelude::*};

use super::card::EventCard;
use crate::view::details::EventDetails;

const PLACEMENT_MARGIN: i32 = 16;
const PLACEMENT_HYSTERESIS: i32 = 64;

type EditHandler = Rc<dyn Fn(ItemId, EditScope)>;
type DeleteHandler = Rc<dyn Fn(ItemId, bool, String)>;
type RespondHandler = Rc<dyn Fn(ItemId, CalPartStat)>;
type OccurrenceHandler = Rc<dyn Fn(ItemId, OccurrenceAction)>;
type TaskHandler = Rc<dyn Fn(ItemId, TaskAction)>;
type ReminderHandler = Rc<dyn Fn(UpdatePersonalAlarms)>;

/// Owns the shared details popup for the calendar.
#[derive(Default)]
pub(crate) struct DetailsController {
    edit: RefCell<Option<EditHandler>>,
    delete: RefCell<Option<DeleteHandler>>,
    respond: RefCell<Option<RespondHandler>>,
    occurrence: RefCell<Option<OccurrenceHandler>>,
    task: RefCell<Option<TaskHandler>>,
    reminders: RefCell<Option<ReminderHandler>>,
    popover: RefCell<Option<EventDetails>>,
    presentation: RefCell<Option<EventDetails>>,
    parent: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    source: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    retained: Cell<bool>,
    anchor: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    minimum_height: Cell<i32>,
    width: Cell<i32>,
    centered: Cell<bool>,
    force_centered: Cell<bool>,
    rejected: Cell<bool>,
    owner_hidden: Cell<bool>,
}

impl DetailsController {
    pub fn connect_edit(&self, callback: impl Fn(ItemId, EditScope) + 'static) {
        self.edit.replace(Some(Rc::new(callback)));
    }

    pub fn connect_delete(&self, callback: impl Fn(ItemId, bool, String) + 'static) {
        self.delete.replace(Some(Rc::new(callback)));
    }

    pub fn connect_respond(&self, callback: impl Fn(ItemId, CalPartStat) + 'static) {
        self.respond.replace(Some(Rc::new(callback)));
    }

    fn bind_actions(&self, popover: &EventDetails) {
        if let Some(callback) = self.reminders.borrow().clone() {
            popover.connect_reminders(move |command| callback(command));
        }
        if let Some(callback) = self.task.borrow().clone() {
            popover.connect_task(move |item, action| callback(item, action));
        }

        if let Some(callback) = self.occurrence.borrow().clone() {
            popover.connect_occurrence(move |item, action| callback(item, action));
        }

        if let Some(callback) = self.respond.borrow().clone() {
            popover.connect_respond(move |item, response| callback(item, response));
        }

        if let Some(callback) = self.edit.borrow().clone() {
            popover.connect_edit(move |item, scope| callback(item, scope));
        }

        if let Some(callback) = self.delete.borrow().clone() {
            popover
                .connect_delete(move |item, recurrent, summary| callback(item, recurrent, summary));
        }
    }

    pub fn connect_occurrence(&self, callback: impl Fn(ItemId, OccurrenceAction) + 'static) {
        self.occurrence.replace(Some(Rc::new(callback)));
    }

    pub fn connect_task(&self, callback: impl Fn(ItemId, TaskAction) + 'static) {
        self.task.replace(Some(Rc::new(callback)));
    }

    pub fn connect_reminders(&self, callback: impl Fn(UpdatePersonalAlarms) + 'static) {
        self.reminders.replace(Some(Rc::new(callback)));
    }

    pub fn retain_reminders(&self) -> Option<EventDetails> {
        let popover = self.presentation.borrow().clone()?;
        self.retained.set(true);
        Some(popover)
    }

    pub fn is_current(&self, popover: &EventDetails) -> bool {
        self.presentation.borrow().as_ref() == Some(popover)
    }

    fn connect_keys(self: &Rc<Self>, popover: &EventDetails) {
        // Native popovers handle Escape themselves, so dismiss through the controller before
        // GTK's animated popdown can be mistaken for a rejected native allocation.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);

        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |keys, key, _, _| {
            if key == gtk::gdk::Key::Escape
                && keys
                    .widget()
                    .and_then(|widget| widget.root())
                    .and_downcast::<gtk::Window>()
                    .is_some_and(|window| !super::editor::has_nested_popup(&window))
                && let Some(controller) = weak.upgrade()
            {
                controller.close();
                return glib::Propagation::Stop;
            }

            glib::Propagation::Proceed
        });
        popover.add_controller(keys);
    }

    fn remeasure(&self, popover: &EventDetails) {
        if self.owner_hidden.get() {
            return;
        }

        let (width, height) = popover.placement_size();
        self.width.set(width);
        self.minimum_height.set(height);
        self.place(popover);
        popover.present();
    }

    pub fn close(&self) {
        self.presentation.take();
        self.anchor.take();
        self.parent.take();
        self.source.take();
        self.retained.set(false);

        if let Some(popover) = self.popover.take() {
            popover.popdown();
            Self::defer_unparent(&popover);
        }
    }

    /// Hides or restores the popup with its owner.
    pub fn set_owner_visible(self: &Rc<Self>, visible: bool) {
        if self.owner_hidden.replace(!visible) == !visible {
            return;
        }

        let Some(popover) = self.popover.borrow().clone() else {
            return;
        };

        if !visible {
            self.rejected.set(false);
            popover.set_visible(false);
        } else if !popover.is_visible() && popover.parent().is_some_and(|parent| parent.is_mapped())
        {
            self.remeasure(&popover);
            popover.popup();
            self.check_mapping(&popover);
        }
    }

    pub fn close_for(&self, parent: &gtk::Widget) {
        if !self.retained.get() && self.is_source(parent) {
            self.close();
        }
    }

    fn is_source(&self, widget: &gtk::Widget) -> bool {
        self.source
            .borrow()
            .as_ref()
            .and_then(|source| source.upgrade())
            .as_ref()
            == Some(widget)
    }

    fn is_parent(&self, parent: &gtk::Widget) -> bool {
        self.parent
            .borrow()
            .as_ref()
            .and_then(|parent| parent.upgrade())
            .as_ref()
            == Some(parent)
    }

    pub fn allocate(&self, parent: &gtk::Widget) {
        if self.owner_hidden.get() {
            return;
        }

        if self.is_parent(parent) || self.is_source(parent) {
            let popover = self.popover.borrow().clone();
            if let Some(popover) = popover {
                self.place(&popover);
                popover.present();
            }
        }
    }

    pub fn show(self: &Rc<Self>, parent: &gtk::Widget, button: &EventCard, locale: &dyn Locale) {
        let Some(event) = button.event() else { return };

        let popover = EventDetails::new(&event, locale);
        self.show_details(parent, button.upcast_ref(), popover);
    }

    pub fn show_details(
        self: &Rc<Self>,
        parent: &gtk::Widget,
        button: &gtk::Widget,
        popover: EventDetails,
    ) {
        self.show_popup(parent, button, popover);
    }

    fn show_popup(
        self: &Rc<Self>,
        parent: &gtk::Widget,
        anchor: &gtk::Widget,
        popover: EventDetails,
    ) {
        self.close();
        self.bind_actions(&popover);
        self.connect_keys(&popover);
        self.source.replace(Some(parent.downgrade()));

        // A page-owned popup keeps its native surface when refreshing replaces its event widget.
        let page = parent.ancestor(super::page::CalendarPage::static_type());
        let parent = page.as_ref().unwrap_or(parent);
        popover.set_parent(parent);
        if let Some(page) = parent
            .clone()
            .downcast::<super::page::CalendarPage>()
            .ok()
            .or_else(|| {
                parent
                    .ancestor(super::page::CalendarPage::static_type())
                    .and_downcast::<super::page::CalendarPage>()
            })
        {
            popover.bind_editor_availability(&page);
        }

        // Measure once before changing arrow and width requests for centered placement.
        let (width, minimum_height) = popover.placement_size();
        self.width.set(width);
        self.minimum_height.set(minimum_height);
        self.centered.set(false);
        self.force_centered.set(false);
        self.rejected.set(false);
        self.parent.replace(Some(parent.downgrade()));
        self.anchor.replace(Some(anchor.downgrade()));
        self.place(&popover);

        let weak = Rc::downgrade(self);
        popover.connect_closed(move |closed| {
            let weak = weak.clone();
            let closed = closed.clone();
            glib::idle_add_local_once(move || {
                if let Some(controller) = weak.upgrade()
                    && controller.popover.borrow().as_ref() == Some(&closed)
                {
                    if controller.owner_hidden.get() {
                        return;
                    }

                    if controller.rejected.replace(false) && controller.retry_centered(&closed) {
                        return;
                    }

                    if closed.is_visible() {
                        // A rejected native allocation can unmap a popup without hiding it.
                        // Recover it once using centered placement rather than leaving it open
                        // in GTK's state but absent from the display.
                        if !closed.is_mapped() {
                            controller.retry_centered(&closed);
                        }
                        return;
                    }

                    controller.popover.take();
                    controller.presentation.take();
                    controller.anchor.take();
                    controller.parent.take();
                    controller.source.take();
                    controller.retained.set(false);
                }

                if closed.parent().is_some() {
                    closed.unparent();
                }
            });
        });

        self.popover.replace(Some(popover.clone()));
        self.presentation.replace(Some(popover.clone()));

        let weak = Rc::downgrade(self);
        let previous_layout = Cell::new(None);
        popover.add_tick_callback(move |popover, _| {
            let Some(controller) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };

            if controller.popover.borrow().as_ref() != Some(popover) {
                return glib::ControlFlow::Break;
            }

            let layout = popover.root().map(|root| {
                let (width, height) = popover.placement_size();
                (height, width, root.height(), height)
            });
            if previous_layout.replace(layout) != layout && layout.is_some() {
                controller.remeasure(popover);
            }

            glib::ControlFlow::Continue
        });

        if !self.owner_hidden.get() {
            popover.popup();
        }

        if let Some(surface) = popover.surface() {
            let weak = Rc::downgrade(self);
            let popup = popover.downgrade();
            surface.connect_layout(move |_, _, height| {
                if let (Some(controller), Some(popup)) = (weak.upgrade(), popup.upgrade())
                    && controller.popover.borrow().as_ref() == Some(&popup)
                    && !controller.owner_hidden.get()
                    && height < controller.minimum_height.get()
                {
                    // GTK emits `closed` for undersized native allocations as well as actual
                    // dismissals. Mark the former before deferred cleanup decides to recover.
                    controller.rejected.set(true);
                }
            });
        }

        self.check_mapping(&popover);
    }

    pub fn is_open(&self) -> bool {
        self.popover.borrow().is_some()
    }

    #[cfg(test)]
    pub fn current(&self) -> Option<EventDetails> {
        self.popover.borrow().clone()
    }

    fn defer_unparent(popover: &EventDetails) {
        // GTK can still use a closed popover until the current GTK call returns.
        let popover = popover.clone();
        glib::idle_add_local_once(move || {
            if popover.parent().is_some() {
                popover.unparent();
            }
        });
    }

    /// Centers the popup when its event is off-screen or anchored placement cannot fit.
    fn place(&self, popover: &EventDetails) {
        let Some(parent) = self
            .parent
            .borrow()
            .as_ref()
            .and_then(|parent| parent.upgrade())
        else {
            return;
        };

        let button = self
            .anchor
            .borrow()
            .as_ref()
            .and_then(|anchor| anchor.upgrade());
        let Some(root) = parent.root().and_downcast::<gtk::Window>() else {
            return;
        };

        let bounds = button
            .as_ref()
            .and_then(|button| button.compute_bounds(&root));

        // Only the intersection of all enclosing viewports is a usable popup anchor.
        let mut visible = Some(graphene::Rect::new(
            0.0,
            0.0,
            root.width() as f32,
            root.height() as f32,
        ));
        let mut ancestor = button.and_then(|button| button.parent());
        while let Some(widget) = ancestor {
            if widget.is::<gtk::ScrolledWindow>() {
                visible = visible.and_then(|clip| {
                    widget
                        .compute_bounds(&root)
                        .and_then(|bounds| clip.intersection(&bounds))
                });
            }

            ancestor = widget.parent();
        }

        let visible_anchor = bounds.and_then(|bounds| {
            visible
                .and_then(|clip| clip.intersection(&bounds))
                .filter(|bounds| bounds.width() > 0.0 && bounds.height() > 0.0)
        });
        let above = visible_anchor.map_or(0, |bounds| bounds.y().floor() as i32);
        let below = visible_anchor.map_or(0, |bounds| {
            root.height() - (bounds.y() + bounds.height()).ceil() as i32
        });
        let centered = visible_anchor.is_none()
            || self.force_centered.get()
            || details_should_center(
                above.max(below),
                self.minimum_height.get(),
                self.centered.get(),
            );
        self.centered.set(centered);
        if !centered {
            let Some(bounds) = visible_anchor else { return };
            let Some(point) =
                root.compute_point(&parent, &graphene::Point::new(bounds.x(), bounds.y()))
            else {
                return;
            };

            let pointing_to = gtk::Allocation::new(
                point.x().round() as i32,
                point.y().round() as i32,
                bounds.width().ceil() as i32,
                bounds.height().ceil() as i32,
            );
            if popover.pointing_to() != (true, pointing_to) {
                popover.set_pointing_to(Some(&pointing_to));
            }
            popover.set_has_arrow(true);
            popover.set_offset(0, 0);
            let details_position = if self.retained.get()
                && popover.position() == gtk::PositionType::Top
                && above >= self.minimum_height.get() + PLACEMENT_MARGIN
            {
                gtk::PositionType::Top
            } else {
                gtk::PositionType::Bottom
            };
            popover.set_position(details_position);
        } else {
            // Side placement can slide vertically across the monitor, avoiding GTK dismissal
            // when an above/below popup cannot meet its minimum height. Center on the window.
            popover.set_has_arrow(false);
            popover.set_position(gtk::PositionType::Right);
            let width = self.width.get();
            let center_x = bounds.map_or(root.width() / 2, |bounds| {
                (bounds.x() + bounds.width() / 2.0).round() as i32
            });
            let x = if width > root.width() {
                (root.width() - width) / 2
            } else {
                (center_x - width / 2).clamp(0, root.width() - width)
            };
            // GDK clips anchors to the native surface, so use an in-window point.
            let Some(point) = root.compute_point(
                &parent,
                &graphene::Point::new((x + width / 2) as f32, (root.height() / 2) as f32),
            ) else {
                return;
            };

            let pointing_to =
                gtk::Allocation::new(point.x().round() as i32, point.y().round() as i32, 1, 1);
            if popover.pointing_to() != (true, pointing_to) {
                popover.set_pointing_to(Some(&pointing_to));
            }
            popover.set_offset(-width / 2 - 1, 0);
        }
    }

    fn retry_centered(self: &Rc<Self>, popover: &EventDetails) -> bool {
        let mapped = self
            .parent
            .borrow()
            .as_ref()
            .and_then(|parent| parent.upgrade())
            .is_some_and(|parent| parent.is_mapped());
        if self.force_centered.get() || !mapped || self.popover.borrow().as_ref() != Some(popover) {
            return false;
        }

        // The compositor also constrains placement to the monitor. Once rejected, stay centered.
        self.force_centered.set(true);
        self.centered.set(true);
        self.place(popover);
        popover.popup();
        self.check_mapping(popover);

        true
    }

    fn check_mapping(self: &Rc<Self>, popover: &EventDetails) {
        let weak = Rc::downgrade(self);
        let popover = popover.clone();
        glib::idle_add_local_once(move || {
            let Some(controller) = weak.upgrade() else {
                return;
            };

            if controller.popover.borrow().as_ref() == Some(&popover)
                && !controller.owner_hidden.get()
                && !popover.is_mapped()
                && popover.is_visible()
                && !controller.retry_centered(&popover)
            {
                controller.close();
            }
        });
    }
}

fn details_should_center(available: i32, minimum: i32, centered: bool) -> bool {
    let threshold = minimum + PLACEMENT_MARGIN + if centered { PLACEMENT_HYSTERESIS } else { 0 };
    available < threshold
}

#[cfg(test)]
mod tests {
    use std::{rc::Rc, time::Duration};

    use gtk::prelude::*;

    use super::details_should_center;

    #[test]
    #[ignore = "requires a GTK display"]
    fn escape_dismisses_details_without_reopening_them() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(true);
        let parent: crate::view::page::CalendarPage = gtk::glib::Object::new();
        let window = gtk::Window::builder()
            .child(&parent)
            .default_width(1000)
            .default_height(900)
            .build();
        let anchor = gtk::Button::with_label("Event");
        parent.append(&anchor);
        let controller = Rc::new(super::DetailsController::default());
        let drain = || {
            for _ in 0..35 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        window.present();
        drain();
        for animations in [false, true] {
            gtk::Settings::default()
                .unwrap()
                .set_gtk_enable_animations(animations);
            let popup: crate::view::details::EventDetails = gtk::glib::Object::new();
            popup.set_child(Some(&gtk::Button::with_label("Details")));
            controller.show_details(anchor.upcast_ref(), anchor.upcast_ref(), popup.clone());
            drain();
            let keys = popup
                .observe_controllers()
                .iter::<gtk::glib::Object>()
                .filter_map(Result::ok)
                .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
                .unwrap();
            assert!(keys.emit_by_name::<bool>(
                "key-pressed",
                &[
                    &gtk::gdk::Key::Escape,
                    &0u32,
                    &gtk::gdk::ModifierType::empty(),
                ],
            ));
            assert!(controller.current().is_none());
            drain();
            controller.allocate(parent.upcast_ref());
            drain();
            assert!(!popup.is_mapped(), "closed details must not reopen");
            assert!(popup.parent().is_none());
            assert!(controller.current().is_none());
        }
        window.close();
        drain();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn item_editor_window_fits_errors_and_keeps_actions_visible() {
        use crate::model::editor::{EditorCalendar, EditorOptions};

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(true);
        let parent: crate::view::page::CalendarPage = gtk::glib::Object::new();
        let window = gtk::Window::builder()
            .child(&parent)
            .default_width(1200)
            .default_height(1000)
            .build();
        window.present();
        let editor = crate::view::editor::ItemEditor::new(
            1,
            EditorOptions {
                kind: eventix_ical::objects::CalCompType::Event,
                calendars: vec![EditorCalendar {
                    id: "test".into(),
                    name: "Test".into(),
                    background: "#27c281".into(),
                    personal: false,
                }],
                last_calendar: None,
                locations: vec![],
                contacts: vec![],
                start: "2026-10-15T10:00:00".parse().unwrap(),
                locale: eventix_locale::default(),
            },
        );
        let editor_window = editor.create_window(&window);
        editor_window.present();
        let settle = || {
            for _ in 0..40 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        settle();
        let clean = editor_window.height();
        editor.show_error("Short error");
        settle();
        let short = editor_window.height();
        assert!(short > clean);
        editor.show_error(&"A longer error message with details. ".repeat(30));
        settle();
        assert!(editor_window.height() > short);
        let bounds = editor.save.compute_bounds(&editor_window).unwrap();
        assert!(bounds.y() >= 0.0);
        assert!(bounds.y() + bounds.height() <= editor_window.height() as f32);
        editor.show_error("Short error");
        settle();
        assert_eq!(editor_window.height(), short);
        editor.clear_errors();
        settle();
        assert_eq!(editor_window.height(), clean);
        editor_window.destroy();
        window.close();
    }

    #[test]
    fn placement_requires_extra_room_before_leaving_centered_mode() {
        let mut centered = false;
        for available in [515, 520, 514, 530, 516, 560, 579] {
            centered = details_should_center(available, 500, centered);
            assert!(centered, "placement changed inside the hysteresis band");
        }
        centered = details_should_center(580, 500, centered);
        assert!(!centered);
        for available in [579, 530, 516] {
            centered = details_should_center(available, 500, centered);
            assert!(!centered, "anchored placement changed while it still fits");
        }
        assert!(details_should_center(515, 500, centered));
    }
}
