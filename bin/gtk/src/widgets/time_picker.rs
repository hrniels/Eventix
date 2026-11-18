// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::Cell, sync::OnceLock};

use chrono::{NaiveTime, Timelike};
use eventix_locale::Locale;
use gtk::{glib, prelude::*, subclass::prelude::*};

const FACE_SIZE: i32 = 228;
const CENTER: f64 = FACE_SIZE as f64 / 2.0;
const OUTER_RADIUS: f64 = 92.0;
const INNER_RADIUS: f64 = 58.0;
const TICK_SIZE: i32 = 30;
const RING_BOUNDARY: f64 = (INNER_RADIUS + OUTER_RADIUS) / 2.0;

mod imp {
    use super::*;

    resource_template! { "widgets/time_picker.ui",
    pub struct TimePicker {
        pub time: Cell<NaiveTime>,
        pub minutes: Cell<bool>,
        pub dragging: Cell<bool>,
        pub face_minutes: Cell<Option<bool>>,
        #[template_child]
        pub hours: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub minutes_button: gtk::TemplateChild<gtk::Button>,
        #[template_child]
        pub face: gtk::TemplateChild<gtk::Fixed>,
    }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TimePicker {
        const NAME: &'static str = "EventixTimePicker";
        type Type = super::TimePicker;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for TimePicker {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("time-selected").build()])
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            // Keep the face dimensions shared with hit testing and hand drawing.
            self.face.set_size_request(FACE_SIZE, FACE_SIZE);
            obj.connect_drag();
            let click = gtk::GestureClick::new();
            let weak = obj.downgrade();
            click.connect_released(move |_, _, x, y| {
                let Some(obj) = weak.upgrade() else { return };
                let face = &obj.imp().face;
                if face.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() != Some(face.upcast_ref()) {
                    return;
                }
                let dx = x - CENTER;
                let dy = y - CENTER;
                let radius = dx.hypot(dy);
                if !(INNER_RADIUS / 2.0..=CENTER).contains(&radius) {
                    return;
                }
                let turn = dx.atan2(-dy).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU;
                let value = if obj.imp().minutes.get() {
                    (turn * 60.0).round() as u32 % 60
                } else {
                    let hour = (turn * 12.0).round() as u32 % 12;
                    if radius < RING_BOUNDARY {
                        if hour == 0 { 12 } else { hour }
                    } else if hour == 0 {
                        0
                    } else {
                        hour + 12
                    }
                };
                obj.choose(value);
            });
            self.face.add_controller(click);
            for (button, minutes) in [(&self.hours, false), (&self.minutes_button, true)] {
                let weak = obj.downgrade();
                button.connect_clicked(move |_| {
                    if let Some(obj) = weak.upgrade() {
                        obj.imp().minutes.set(minutes);
                        obj.refresh();
                    }
                });
            }
            obj.set_locale(eventix_locale::default().as_ref());
            obj.refresh();
        }

        fn dispose(&self) {
            self.dispose_template();
        }
    }

    impl WidgetImpl for TimePicker {}
    impl BoxImpl for TimePicker {}
}

glib::wrapper! {
    /// A 24-hour clock face that confirms after choosing hours and minutes.
    pub struct TimePicker(ObjectSubclass<imp::TimePicker>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl Default for TimePicker {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl TimePicker {
    /// Updates the accessible clock component labels without changing the time.
    pub fn set_locale(&self, locale: &dyn Locale) {
        for (button, label) in [
            (&self.imp().hours, "Hours"),
            (&self.imp().minutes_button, "Minutes"),
        ] {
            let text = locale.translate(label);
            button.set_tooltip_text(Some(text));
            button.update_property(&[gtk::accessible::Property::Label(text)]);
        }
    }

    /// Sets the time and resets the clock to hour selection without emitting a signal.
    pub fn select_time(&self, time: NaiveTime) {
        self.imp().dragging.set(false);
        self.imp().time.set(time);
        self.imp().minutes.set(false);
        self.refresh();
    }

    /// Returns the currently selected time.
    pub fn time(&self) -> NaiveTime {
        self.imp().time.get()
    }

    /// Switches the clock face between hours and minutes without changing the time or focus.
    pub fn show_minutes(&self, minutes: bool) {
        self.imp().minutes.set(minutes);
        self.refresh();
    }

    /// Connects a callback invoked when the minute selection confirms the time.
    pub fn connect_time_selected(&self, callback: impl Fn(&Self) + 'static) {
        self.connect_local("time-selected", false, move |values| {
            callback(&values[0].get::<Self>().expect("time-selected sender"));
            None
        });
    }

    fn connect_drag(&self) {
        // Capture minute presses before the five-minute tick buttons can snap or confirm them.
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        drag.connect_drag_begin(move |drag, x, y| {
            let Some(obj) = weak.upgrade() else { return };
            if !obj.imp().minutes.get() || (x - CENTER).hypot(y - CENTER) > CENTER {
                drag.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            obj.imp().dragging.set(true);
            drag.set_state(gtk::EventSequenceState::Claimed);
            obj.drag_minute(x, y);
        });
        let weak = self.downgrade();
        drag.connect_drag_update(move |drag, dx, dy| {
            if let Some(obj) = weak.upgrade()
                && let Some((x, y)) = drag.start_point()
            {
                obj.drag_minute(x + dx, y + dy);
            }
        });
        let weak = self.downgrade();
        drag.connect_drag_end(move |drag, dx, dy| {
            if let Some(obj) = weak.upgrade()
                && obj.imp().dragging.get()
            {
                if let Some((x, y)) = drag.start_point() {
                    obj.drag_minute(x + dx, y + dy);
                }
                obj.imp().dragging.set(false);
                obj.emit_by_name::<()>("time-selected", &[]);
            }
        });
        let weak = self.downgrade();
        drag.connect_cancel(move |_, _| {
            if let Some(obj) = weak.upgrade() {
                obj.imp().dragging.set(false);
            }
        });
        self.imp().face.add_controller(drag);
    }

    fn drag_minute(&self, x: f64, y: f64) {
        let dx = x - CENTER;
        let dy = y - CENTER;
        if !self.imp().dragging.get() || dx.hypot(dy) < 4.0 {
            return;
        }
        let turn = dx.atan2(-dy).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU;
        let minute = (turn * 60.0).round() as u32 % 60;
        self.imp()
            .time
            .set(self.time().with_minute(minute).unwrap());
        self.refresh();
    }

    fn choose(&self, value: u32) {
        let imp = self.imp();
        let time = self.time();
        let minutes = imp.minutes.get();
        imp.time.set(if minutes {
            time.with_minute(value).unwrap()
        } else {
            time.with_hour(value).unwrap()
        });
        imp.minutes.set(true);
        self.refresh();
        if minutes {
            self.emit_by_name::<()>("time-selected", &[]);
        }
    }

    fn refresh(&self) {
        let imp = self.imp();
        let time = self.time();
        imp.hours.set_label(&format!("{:02}", time.hour()));
        imp.minutes_button
            .set_label(&format!("{:02}", time.minute()));
        for (button, active) in [
            (&imp.hours, !imp.minutes.get()),
            (&imp.minutes_button, imp.minutes.get()),
        ] {
            if active {
                button.add_css_class("suggested-action");
            } else {
                button.remove_css_class("suggested-action");
            }
        }
        let minutes = imp.minutes.get();
        // Preserve pointer targets while dragging; rebuilding ticks cancels GTK's active sequence.
        if imp.face_minutes.get() == Some(minutes) {
            self.refresh_face_selection();
            return;
        }
        imp.face_minutes.set(Some(minutes));
        while let Some(child) = imp.face.first_child() {
            imp.face.remove(&child);
        }
        imp.face.put(&self.clock_hand(), 0.0, 0.0);
        for index in 0..if minutes { 12 } else { 24 } {
            let value = if minutes { index * 5 } else { index };
            let angle = f64::from(index % 12) * std::f64::consts::TAU / 12.0;
            let radius = if !minutes && (1..=12).contains(&index) {
                INNER_RADIUS
            } else {
                OUTER_RADIUS
            };
            let button = gtk::Button::with_label(&format!("{value:02}"));
            button.set_size_request(TICK_SIZE, TICK_SIZE);
            button.set_focus_on_click(false);
            button.set_focusable(false);
            button.add_css_class("ev_time_picker_tick");
            imp.face.put(
                &button,
                CENTER - f64::from(TICK_SIZE) / 2.0 + radius * angle.sin(),
                CENTER - f64::from(TICK_SIZE) / 2.0 - radius * angle.cos(),
            );
            let weak = self.downgrade();
            button.connect_clicked(move |_| {
                if let Some(obj) = weak.upgrade() {
                    obj.choose(value);
                }
            });
        }
        self.refresh_face_selection();
    }

    fn refresh_face_selection(&self) {
        let time = self.time();
        let selected = if self.imp().minutes.get() {
            time.minute()
        } else {
            time.hour()
        };
        let mut child = self.imp().face.first_child();
        while let Some(widget) = child {
            if let Some(button) = widget.downcast_ref::<gtk::Button>() {
                let value = button.label().and_then(|label| label.parse::<u32>().ok());
                if value == Some(selected) {
                    button.remove_css_class("flat");
                    button.add_css_class("suggested-action");
                } else {
                    button.remove_css_class("suggested-action");
                    button.add_css_class("flat");
                }
            } else {
                widget.queue_draw();
            }
            child = widget.next_sibling();
        }
    }

    fn clock_hand(&self) -> gtk::DrawingArea {
        let hand = gtk::DrawingArea::new();
        hand.set_size_request(FACE_SIZE, FACE_SIZE);
        hand.set_can_target(false);
        let weak = self.downgrade();
        hand.set_draw_func(move |area, cr, _, _| {
            let Some(obj) = weak.upgrade() else { return };
            let time = obj.time();
            let minutes = obj.imp().minutes.get();
            let value = if minutes {
                time.minute()
            } else {
                time.hour() % 12
            };
            let angle =
                f64::from(value) * std::f64::consts::TAU / if minutes { 60.0 } else { 12.0 };
            let radius = if !minutes && (1..=12).contains(&time.hour()) {
                INNER_RADIUS
            } else {
                OUTER_RADIUS
            };
            let color = area.color();
            cr.set_source_rgb(0.96, 0.96, 0.96);
            cr.arc(CENTER, CENTER, CENTER - 1.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.18);
            cr.set_line_width(1.0);
            cr.arc(CENTER, CENTER, CENTER - 1.0, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            if !minutes {
                cr.arc(CENTER, CENTER, RING_BOUNDARY, 0.0, std::f64::consts::TAU);
                let _ = cr.stroke();
            }
            cr.set_source_rgba(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                0.5,
            );
            cr.set_line_width(2.0);
            cr.move_to(CENTER, CENTER);
            cr.line_to(CENTER + radius * angle.sin(), CENTER - radius * angle.cos());
            let _ = cr.stroke();
            cr.arc(CENTER, CENTER, 3.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        });
        hand
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a GTK display"]
    fn time_picker_precise_minute_drag_confirms_only_on_release() {
        use std::rc::Rc;

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let picker: TimePicker = glib::Object::new();
        let count = Rc::new(Cell::new(0));
        let selected = count.clone();
        picker.connect_time_selected(move |_| selected.set(selected.get() + 1));
        picker.select_time("23:58".parse().unwrap());
        picker.show_minutes(true);
        let face = &picker.imp().face;
        let controllers = face.observe_controllers();
        let drag = (0..controllers.n_items())
            .find_map(|index| controllers.item(index)?.downcast::<gtk::GestureDrag>().ok())
            .unwrap();
        drag.emit_by_name::<()>("drag-begin", &[&CENTER, &0.0_f64]);
        assert!(picker.imp().dragging.get());
        let target = face.last_child().unwrap();
        for minute in [1, 17, 29, 43, 59, 0] {
            let angle = f64::from(minute) * std::f64::consts::TAU / 60.0;
            picker.drag_minute(
                CENTER + OUTER_RADIUS * angle.sin(),
                CENTER - OUTER_RADIUS * angle.cos(),
            );
            assert_eq!(picker.time().minute(), minute);
            assert_eq!(picker.time().hour(), 23);
            assert_eq!(count.get(), 0);
            assert_eq!(face.last_child().unwrap(), target);
        }
        drag.emit_by_name::<()>("drag-end", &[&0.0_f64, &0.0_f64]);
        assert!(!picker.imp().dragging.get());
        assert_eq!(count.get(), 1);
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn time_picker_selects_hours_then_minutes_and_resets_without_confirming() {
        use std::rc::Rc;

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let picker: TimePicker = glib::Object::new();
        let count = Rc::new(Cell::new(0));
        let selected = count.clone();
        picker.connect_time_selected(move |_| selected.set(selected.get() + 1));
        picker.select_time(NaiveTime::from_hms_opt(9, 17, 0).unwrap());
        let window = gtk::Window::builder().child(&picker).build();
        window.present();
        while glib::MainContext::default().iteration(false) {}
        gtk::test_widget_wait_for_draw(&picker);
        let face = &picker.imp().face;
        assert_eq!(face.width_request(), FACE_SIZE);
        let mut child = face.first_child();
        let mut ticks = 0;
        while let Some(widget) = child {
            if let Some(button) = widget.downcast_ref::<gtk::Button>() {
                let value: u32 = button.label().unwrap().parse().unwrap();
                let (x, y) = face.child_position(button);
                let radius = (x + f64::from(button.width()) / 2.0 - CENTER)
                    .hypot(y + f64::from(button.height()) / 2.0 - CENTER);
                let expected = if (1..=12).contains(&value) {
                    INNER_RADIUS
                } else {
                    OUTER_RADIUS
                };
                assert!((radius - expected).abs() < 0.001);
                assert_eq!(button.width_request(), TICK_SIZE);
                assert!(!button.is_focusable());
                ticks += 1;
            }
            child = widget.next_sibling();
        }
        assert_eq!(ticks, 24);
        picker.choose(23);
        assert_eq!(picker.time().hour(), 23);
        assert_eq!(picker.time().minute(), 17);
        assert_eq!(count.get(), 0);
        picker.choose(55);
        assert_eq!(picker.time().minute(), 55);
        assert_eq!(count.get(), 1);
        picker.select_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        assert!(!picker.imp().minutes.get());
        picker.choose(0);
        picker.choose(0);
        assert_eq!(count.get(), 2);
        window.close();
    }
}
