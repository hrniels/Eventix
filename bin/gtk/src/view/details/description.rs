// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
};

use eventix_locale::Locale;
use gtk::{gdk, gio, glib, prelude::*, subclass::prelude::*};

use super::text_links;

type Activate = Rc<dyn Fn(&Description, &str)>;

struct Link {
    start: i32,
    end: i32,
    uri: String,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Description {
        pub(super) links: RefCell<Vec<Link>>,
        pub preview: RefCell<String>,
        pub tag: OnceCell<gtk::TextTag>,
        pub hover_tag: OnceCell<gtk::TextTag>,
        pub hovered: Cell<Option<(i32, i32)>>,
        pub focus_tag: OnceCell<gtk::TextTag>,
        pub(super) activate: RefCell<Option<Activate>>,
        pub pressed: RefCell<Option<String>>,
        pub context_uri: RefCell<Option<String>>,
        pub menu: OnceCell<gio::Menu>,
        pub theme_handlers: RefCell<Vec<(gtk::Settings, glib::SignalHandlerId)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Description {
        const NAME: &'static str = "EventixDescription";
        type Type = super::Description;
        type ParentType = gtk::TextView;
    }

    impl ObjectImpl for Description {
        fn dispose(&self) {
            for (settings, handler) in self.theme_handlers.take() {
                settings.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for Description {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let metrics = obj.pango_context().metrics(None, None);
            let width = (metrics.approximate_char_width() * 42 / gtk::pango::SCALE).max(1);
            if orientation == gtk::Orientation::Horizontal {
                return (0, width, -1, -1);
            }

            // Size only a bounded preview; the TextView lays out the actual document lazily.
            // Short descriptions remain compact without scanning a document-sized paragraph.
            let layout = obj.create_pango_layout(Some(&self.preview.borrow()));
            let width = if for_size < 0 { width } else { for_size.max(1) };
            layout.set_width(width.saturating_mul(gtk::pango::SCALE));
            layout.set_wrap(gtk::pango::WrapMode::WordChar);
            layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
            layout.set_height(150 * gtk::pango::SCALE);
            let height = layout.pixel_size().1.min(150);
            (height, height, -1, -1)
        }
    }

    impl TextViewImpl for Description {}
}

glib::wrapper! {
    pub struct Description(ObjectSubclass<imp::Description>)
        @extends gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Description {
    pub(super) fn new(text: &str, locale: &dyn Locale) -> Self {
        let error_title = locale.translate("Could not open link").to_owned();
        Self::with_activate(text, locale, move |view, uri| {
            let parent = view.root().and_downcast::<gtk::Window>();
            let error_parent = parent.clone();
            let error_title = error_title.clone();
            gtk::UriLauncher::new(uri).launch(
                parent.as_ref(),
                None::<&gio::Cancellable>,
                move |result| {
                    if let Err(error) = result {
                        tracing::warn!(%error, "could not open description URI");
                        gtk::AlertDialog::builder()
                            .message(&error_title)
                            .detail(error.message())
                            .build()
                            .show(error_parent.as_ref());
                    }
                },
            );
        })
    }

    fn with_activate(
        text: &str,
        locale: &dyn Locale,
        activate: impl Fn(&Self, &str) + 'static,
    ) -> Self {
        let view: Self = glib::Object::builder()
            .property("editable", false)
            .property("cursor-visible", false)
            .property("accepts-tab", false)
            .property("wrap-mode", gtk::WrapMode::WordChar)
            .property("hexpand", true)
            .build();
        view.add_css_class("ev_details_description");
        view.update_property(&[gtk::accessible::Property::Label(
            locale.translate("Description"),
        )]);
        view.imp().activate.replace(Some(Rc::new(activate)));
        view.imp().preview.replace(
            text.split_inclusive('\n')
                .take(10)
                .flat_map(str::chars)
                .take(1000)
                .collect(),
        );
        view.buffer().set_text(text);
        view.add_links(text);
        view.setup_menu(locale);
        view.setup_pointer();
        view.setup_keys();
        view.setup_theme();
        let weak = view.downgrade();
        view.buffer().connect_mark_set(move |buffer, _, mark| {
            if mark == &buffer.get_insert()
                && let Some(view) = weak.upgrade()
            {
                view.clear_link_focus();
            }
        });
        view.connect_has_focus_notify(|view| {
            if !view.has_focus() {
                view.clear_link_focus();
            }
        });
        view
    }

    fn add_links(&self, text: &str) {
        let buffer = self.buffer();
        let tag = gtk::TextTag::builder()
            .underline(gtk::pango::Underline::Single)
            .build();
        buffer.tag_table().add(&tag);
        let mut previous_end = 0;
        let mut offset = 0;
        let mut links = self.imp().links.borrow_mut();
        for link in text_links::links(text) {
            // GtkTextIter offsets count Unicode characters, whereas regex offsets count bytes.
            let prefix = text[previous_end..link.start].chars().count();
            let length = link.text.chars().count();
            let (Ok(prefix), Ok(length)) = (i32::try_from(prefix), i32::try_from(length)) else {
                break;
            };
            let Some(start) = i32::checked_add(offset, prefix) else {
                break;
            };
            let Some(end) = i32::checked_add(start, length) else {
                break;
            };
            buffer.apply_tag(
                &tag,
                &buffer.iter_at_offset(start),
                &buffer.iter_at_offset(end),
            );
            links.push(Link {
                start,
                end,
                uri: link.uri,
            });
            offset = end;
            previous_end = link.end;
        }
        let _ = self.imp().tag.set(tag);
        let hover = gtk::TextTag::new(None);
        buffer.tag_table().add(&hover);
        let _ = self.imp().hover_tag.set(hover);
        let focus = gtk::TextTag::new(None);
        buffer.tag_table().add(&focus);
        let _ = self.imp().focus_tag.set(focus);
    }

    fn update_link_color(&self) {
        if let Some(tag) = self.imp().tag.get() {
            tag.set_foreground_rgba(Some(&crate::style::color(self, "ev_link")));
        }
        if let Some(tag) = self.imp().hover_tag.get() {
            tag.set_foreground_rgba(Some(&crate::style::color(self, "ev_link_hover")));
        }
        if let Some(tag) = self.imp().focus_tag.get() {
            tag.set_background_rgba(Some(&crate::style::color(self, "theme_selected_bg_color")));
            tag.set_foreground_rgba(Some(&crate::style::color(self, "theme_selected_fg_color")));
        }
    }

    fn clear_link_focus(&self) {
        if let Some(tag) = self.imp().focus_tag.get() {
            let buffer = self.buffer();
            buffer.remove_tag(tag, &buffer.start_iter(), &buffer.end_iter());
        }
    }

    fn setup_theme(&self) {
        self.connect_realize(|view| view.update_link_color());
        if let Some(settings) = gtk::Settings::default() {
            let weak = self.downgrade();
            let theme = settings.connect_gtk_theme_name_notify(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.update_link_color();
                }
            });
            let weak = self.downgrade();
            let dark = settings.connect_gtk_application_prefer_dark_theme_notify(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.update_link_color();
                }
            });
            self.imp()
                .theme_handlers
                .borrow_mut()
                .extend([(settings.clone(), theme), (settings, dark)]);
        }
    }

    fn uri_at_offset(&self, offset: i32) -> Option<String> {
        self.imp()
            .links
            .borrow()
            .iter()
            .find(|link| (link.start..link.end).contains(&offset))
            .map(|link| link.uri.clone())
    }

    fn uri_at_point(&self, x: f64, y: f64) -> Option<String> {
        self.uri_at_offset(self.offset_at_point(x, y)?)
    }

    fn offset_at_point(&self, x: f64, y: f64) -> Option<i32> {
        let (x, y) = self.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        // Unlike iter_at_location(), this returns the glyph under the pointer rather than
        // the nearest caret, which would reject the trailing half of each character.
        let (iter, _) = self.iter_at_position(x, y)?;
        Some(iter.offset())
    }

    fn update_link_hover(&self, offset: Option<i32>) {
        let range = offset.and_then(|offset| {
            self.imp()
                .links
                .borrow()
                .iter()
                .find(|link| (link.start..link.end).contains(&offset))
                .map(|link| (link.start, link.end))
        });
        let previous = self.imp().hovered.replace(range);
        if previous == range {
            return;
        }
        if let Some(tag) = self.imp().hover_tag.get() {
            let buffer = self.buffer();
            if let Some((start, end)) = previous {
                buffer.remove_tag(
                    tag,
                    &buffer.iter_at_offset(start),
                    &buffer.iter_at_offset(end),
                );
            }
            if let Some((start, end)) = range {
                buffer.apply_tag(
                    tag,
                    &buffer.iter_at_offset(start),
                    &buffer.iter_at_offset(end),
                );
            }
        }
    }

    fn activate_uri(&self, uri: &str) {
        let callback = self.imp().activate.borrow().clone();
        if let Some(callback) = callback {
            callback(self, uri);
        }
    }

    fn setup_pointer(&self) {
        let motion = gtk::EventControllerMotion::new();
        let weak = self.downgrade();
        motion.connect_motion(move |_, x, y| {
            if let Some(view) = weak.upgrade() {
                let uri = view.uri_at_point(x, y);
                view.update_link_hover(view.offset_at_point(x, y));
                if view.tooltip_text().as_deref() != uri.as_deref() {
                    view.set_tooltip_text(uri.as_deref());
                }
                view.set_cursor_from_name(Some(if uri.is_some() { "pointer" } else { "text" }));
            }
        });
        let weak = self.downgrade();
        motion.connect_leave(move |_| {
            if let Some(view) = weak.upgrade() {
                view.update_link_hover(None);
                view.set_cursor_from_name(None);
                view.set_tooltip_text(None);
            }
        });
        self.add_controller(motion);

        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        click.connect_pressed(move |_, presses, x, y| {
            if let Some(view) = weak.upgrade() {
                view.clear_link_focus();
                let uri = (presses == 1 && !view.buffer().has_selection())
                    .then(|| view.uri_at_point(x, y))
                    .flatten();
                view.imp().pressed.replace(uri);
            }
        });
        let weak = self.downgrade();
        click.connect_released(move |_, presses, x, y| {
            if let Some(view) = weak.upgrade() {
                let pressed = view.imp().pressed.take();
                if presses == 1
                    && !view.buffer().has_selection()
                    && let Some(uri) =
                        pressed.filter(|uri| view.uri_at_point(x, y).as_ref() == Some(uri))
                {
                    view.activate_uri(&uri);
                }
            }
        });
        self.add_controller(click);
    }

    fn setup_menu(&self, locale: &dyn Locale) {
        let menu = gio::Menu::new();
        menu.append(
            Some(locale.translate("Open Link")),
            Some("description.open"),
        );
        menu.append(
            Some(locale.translate("Copy Link Address")),
            Some("description.copy"),
        );
        let _ = self.imp().menu.set(menu);
        let group = gio::SimpleActionGroup::new();
        for name in ["open", "copy"] {
            let action = gio::SimpleAction::new(name, None);
            let weak = self.downgrade();
            action.connect_activate(move |_, _| {
                if let Some(view) = weak.upgrade() {
                    let uri = view.imp().context_uri.borrow().clone();
                    let Some(uri) = uri else { return };
                    if name == "open" {
                        view.activate_uri(&uri);
                    } else {
                        view.clipboard().set_text(&uri);
                    }
                }
            });
            group.add_action(&action);
        }
        self.insert_action_group("description", Some(&group));
        let right = gtk::GestureClick::new();
        right.set_button(3);
        right.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        right.connect_pressed(move |_, _, x, y| {
            if let Some(view) = weak.upgrade() {
                view.set_context_uri(view.uri_at_point(x, y));
            }
        });
        self.add_controller(right);
    }

    fn set_context_uri(&self, uri: Option<String>) {
        let has_uri = uri.is_some();
        self.imp().context_uri.replace(uri);
        self.set_extra_menu(if has_uri { self.imp().menu.get() } else { None });
    }

    fn setup_keys(&self) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            weak.upgrade().map_or(glib::Propagation::Proceed, |view| {
                view.handle_key(key, modifiers)
            })
        });
        self.add_controller(keys);
    }

    fn handle_key(&self, key: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        let buffer = self.buffer();
        let offset = buffer.cursor_position();
        if key == gdk::Key::Menu
            || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK))
        {
            self.set_context_uri(self.uri_at_offset(offset));
        }
        if modifiers.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        ) {
            return glib::Propagation::Proceed;
        }
        if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
            && !buffer.has_selection()
            && let Some(uri) = self.uri_at_offset(offset)
        {
            self.activate_uri(&uri);
            return glib::Propagation::Stop;
        }
        if matches!(key, gdk::Key::Tab | gdk::Key::ISO_Left_Tab) {
            let backwards =
                key == gdk::Key::ISO_Left_Tab || modifiers.contains(gdk::ModifierType::SHIFT_MASK);
            let links = self.imp().links.borrow();
            let next = if backwards {
                links.iter().rev().find(|link| link.start < offset)
            } else {
                links.iter().find(|link| link.start > offset)
            };
            if let Some(link) = next {
                buffer.place_cursor(&buffer.iter_at_offset(link.start));
                if let Some(tag) = self.imp().focus_tag.get() {
                    buffer.apply_tag(
                        tag,
                        &buffer.iter_at_offset(link.start),
                        &buffer.iter_at_offset(link.end),
                    );
                }
                self.scroll_mark_onscreen(&buffer.get_insert());
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        time::{Duration, Instant},
    };

    use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

    use super::Description;

    fn wait_until(ready: impl Fn() -> bool) {
        let start = Instant::now();
        while !ready() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "GTK did not become ready"
            );
            gtk::glib::MainContext::default().iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn scroller(view: &Description) -> gtk::ScrolledWindow {
        let builder = gtk::Builder::from_resource(resource_path!("view/details/scrollers.ui"));
        let scroll: gtk::ScrolledWindow = builder.object("description_scroll").unwrap();
        scroll.set_child(Some(view));
        scroll
    }

    fn point(view: &Description, offset: i32) -> (f64, f64) {
        let rect = view.iter_location(&view.buffer().iter_at_offset(offset));
        let (x, y) = view.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            rect.x() + rect.width() / 2,
            rect.y() + rect.height() / 2,
        );
        (f64::from(x), f64::from(y))
    }

    fn gesture(view: &Description, button: u32) -> gtk::GestureClick {
        view.observe_controllers()
            .iter::<glib::Object>()
            .filter_map(Result::ok)
            .filter_map(|obj| obj.downcast::<gtk::GestureClick>().ok())
            .find(|click| {
                click.button() == button
                    && click.propagation_phase() == gtk::PropagationPhase::Capture
            })
            .unwrap()
    }

    fn check_links(view: &Description, targets: &RefCell<Vec<String>>, text: &str) {
        let buffer = view.buffer();
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            text
        );
        assert!(!view.is_editable() && !view.accepts_tab());
        assert_eq!(view.wrap_mode(), gtk::WrapMode::WordChar);
        assert_eq!(view.imp().links.borrow().len(), 3);
        let first = view.imp().links.borrow()[0].start;
        let end = view.imp().links.borrow()[0].end;
        let second = view.imp().links.borrow()[1].start;
        assert_eq!(
            buffer.text(
                &buffer.iter_at_offset(first),
                &buffer.iter_at_offset(end),
                true
            ),
            "https://example.org/?a=1&b=2"
        );
        assert!(!buffer.iter_at_offset(first).tags().is_empty());
        let link_color = view.imp().tag.get().unwrap().foreground_rgba().unwrap();
        assert_eq!(link_color, crate::style::color(view, "ev_link"));
        assert_ne!(link_color, view.color());

        // Every pixel of each glyph must hit the same link, including its trailing half.
        for offset in first..end {
            let rect = view.iter_location(&buffer.iter_at_offset(offset));
            for dx in 0..rect.width() {
                let (x, y) = view.buffer_to_window_coords(
                    gtk::TextWindowType::Widget,
                    rect.x() + dx,
                    rect.y() + rect.height() / 2,
                );
                assert_eq!(
                    view.uri_at_point(f64::from(x), f64::from(y)).as_deref(),
                    Some("https://example.org/?a=1&b=2"),
                    "link hit missed at character {offset}, pixel {dx}",
                );
            }
        }
        let click = gesture(view, 1);
        let (x, y) = point(view, first + 2);
        let motion = view
            .observe_controllers()
            .iter::<glib::Object>()
            .filter_map(Result::ok)
            .find_map(|obj| obj.downcast::<gtk::EventControllerMotion>().ok())
            .unwrap();
        motion.emit_by_name::<()>("motion", &[&x, &y]);
        let hover_tag = view.imp().hover_tag.get().unwrap();
        assert_eq!(
            hover_tag.foreground_rgba().unwrap(),
            crate::style::color(view, "ev_link_hover")
        );
        assert!(buffer.iter_at_offset(first).has_tag(hover_tag));
        assert!(!buffer.iter_at_offset(second).has_tag(hover_tag));
        let (second_x, second_y) = point(view, second + 2);
        motion.emit_by_name::<()>("motion", &[&second_x, &second_y]);
        assert!(!buffer.iter_at_offset(first).has_tag(hover_tag));
        assert!(buffer.iter_at_offset(second).has_tag(hover_tag));
        motion.emit_by_name::<()>("motion", &[&x, &y]);
        assert_eq!(
            view.tooltip_text().as_deref(),
            Some("https://example.org/?a=1&b=2")
        );
        assert_eq!(view.cursor().unwrap().name().as_deref(), Some("pointer"));
        motion.emit_by_name::<()>("leave", &[]);
        assert!(!buffer.iter_at_offset(first).has_tag(hover_tag));
        assert!(view.cursor().is_none());
        click.emit_by_name::<()>("pressed", &[&1i32, &x, &y]);
        click.emit_by_name::<()>("released", &[&1i32, &x, &y]);
        assert_eq!(*targets.borrow(), ["https://example.org/?a=1&b=2"]);

        // Selecting link text, double-clicking, and releasing away from the press must not open it.
        buffer.select_range(&buffer.iter_at_offset(first), &buffer.iter_at_offset(end));
        click.emit_by_name::<()>("pressed", &[&1i32, &x, &y]);
        click.emit_by_name::<()>("released", &[&1i32, &x, &y]);
        let (selected_start, selected_end) = buffer.selection_bounds().unwrap();
        assert_eq!(
            buffer.text(&selected_start, &selected_end, true),
            "https://example.org/?a=1&b=2"
        );
        buffer.copy_clipboard(&view.clipboard());
        let copied = glib::MainContext::default()
            .block_on(view.clipboard().read_text_future())
            .unwrap();
        assert_eq!(copied.as_deref(), Some("https://example.org/?a=1&b=2"));
        buffer.place_cursor(&buffer.start_iter());
        click.emit_by_name::<()>("pressed", &[&2i32, &x, &y]);
        click.emit_by_name::<()>("released", &[&2i32, &x, &y]);
        click.emit_by_name::<()>("pressed", &[&1i32, &x, &y]);
        click.emit_by_name::<()>("released", &[&1i32, &0.0f64, &0.0f64]);
        assert_eq!(targets.borrow().len(), 1);
        assert!(
            view.uri_at_point(f64::from(view.width() - 1), 1.0)
                .is_none()
        );

        assert_eq!(
            view.handle_key(gdk::Key::Tab, gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
        assert_eq!(buffer.cursor_position(), first);
        assert!(
            buffer
                .iter_at_offset(first)
                .has_tag(view.imp().focus_tag.get().unwrap())
        );
        view.handle_key(gdk::Key::Return, gdk::ModifierType::empty());
        view.handle_key(gdk::Key::Tab, gdk::ModifierType::empty());
        assert_eq!(buffer.cursor_position(), second);
        view.handle_key(gdk::Key::Return, gdk::ModifierType::empty());
        assert_eq!(targets.borrow().last().unwrap(), "mailto:amy@example.org");
        view.handle_key(gdk::Key::Tab, gdk::ModifierType::empty());
        view.handle_key(gdk::Key::Return, gdk::ModifierType::empty());
        assert_eq!(targets.borrow().last().unwrap(), "https://www.example.org");
        assert_eq!(
            view.handle_key(gdk::Key::Tab, gdk::ModifierType::empty()),
            glib::Propagation::Proceed
        );
        view.handle_key(gdk::Key::ISO_Left_Tab, gdk::ModifierType::SHIFT_MASK);
        view.handle_key(gdk::Key::ISO_Left_Tab, gdk::ModifierType::SHIFT_MASK);
        assert_eq!(buffer.cursor_position(), first);
        assert_eq!(
            view.handle_key(gdk::Key::ISO_Left_Tab, gdk::ModifierType::SHIFT_MASK),
            glib::Propagation::Proceed
        );
        let right = gesture(view, 3);
        right.emit_by_name::<()>("pressed", &[&1i32, &x, &y]);
        assert_eq!(view.extra_menu().unwrap().n_items(), 2);
        view.activate_action("description.open", None).unwrap();
        view.activate_action("description.copy", None).unwrap();
        let copied = glib::MainContext::default()
            .block_on(view.clipboard().read_text_future())
            .unwrap();
        assert_eq!(copied.as_deref(), Some("https://example.org/?a=1&b=2"));
        right.emit_by_name::<()>("pressed", &[&1i32, &0.0f64, &0.0f64]);
        assert!(view.extra_menu().is_none());
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn description_links_selection_keyboard_and_compact_scrolling() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gdk::Display::default().unwrap());
        let locale = eventix_locale::default();
        let targets = Rc::new(RefCell::new(Vec::new()));
        let received = targets.clone();
        let text =
            "Grüße 😀 <notes> & https://example.org/?a=1&b=2),\namy@example.org; www.example.org.";
        let view = Description::with_activate(text, locale.as_ref(), move |_, uri| {
            received.borrow_mut().push(uri.to_owned());
        });
        let scroll = scroller(&view);
        let window = gtk::Window::builder()
            .default_width(460)
            .child(&scroll)
            .build();
        window.present();
        wait_until(|| view.is_mapped() && view.width() > 0 && view.height() > 0);
        check_links(&view, &targets, text);
        window.destroy();

        let short = Description::new("One short line", locale.as_ref());
        let scroll = scroller(&short);
        let window = gtk::Window::builder()
            .default_width(460)
            .child(&scroll)
            .build();
        window.present();
        wait_until(|| short.is_mapped() && short.height() > 0);
        assert!(
            scroll.height() < 80,
            "short descriptions must not reserve a 150px area"
        );
        assert!(!scroll.vscrollbar().is_mapped());
        window.destroy();

        let text = "Long description <&> https://example.org\n".repeat(1000);
        let long = Description::new(&text, locale.as_ref());
        let scroll = scroller(&long);
        let window = gtk::Window::builder()
            .default_width(460)
            .child(&scroll)
            .build();
        window.present();
        wait_until(|| long.is_mapped() && long.height() > 0);
        assert!(scroll.height() <= 150);
        wait_until(|| scroll.vscrollbar().is_mapped());
        assert!(scroll.is_overlay_scrolling());
        wait_until(|| {
            let end = long.iter_location(&long.buffer().end_iter());
            let adjustment = scroll.vadjustment();
            adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(0.0));
            long.queue_draw();
            let visible = long.visible_rect();
            end.y() >= visible.y() && end.y() + end.height() <= visible.y() + visible.height() + 1
        });
        assert_eq!(
            long.buffer()
                .text(&long.buffer().start_iter(), &long.buffer().end_iter(), true),
            text
        );
        window.destroy();
    }
}
