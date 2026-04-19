// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::{CalCompType, DateContext};
use eventix_locale::Locale;
use gtk::gdk::RGBA;
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk::gio;
use gtk::glib;
use gtk::{
    prelude::*, Align, Box as GtkBox, Button, DropDown, Image, Label, ListItem, Orientation,
    SignalListItemFactory, StringObject, Window,
};
use std::cell::RefCell;
use std::rc::Rc;
use xdg::BaseDirectories;

use crate::model::{ImportCalendar, ImportModel};

// Stores the calendar id for each entry in the DropDown model.
struct CalEntry {
    id: String,
    icon: Pixbuf,
}

pub struct ImportView {
    window: Window,
}

impl ImportView {
    /// Initialize GTK
    pub fn init() {
        gtk::init().expect("Failed to initialize GTK.");
    }

    pub fn new<T>(
        model: ImportModel,
        xdg: &BaseDirectories,
        locale: &dyn Locale,
        data: T,
        import: fn(T, String) -> anyhow::Result<()>,
    ) -> Self
    where
        T: 'static,
    {
        // Create the top-level window
        let window = Window::builder()
            .title(locale.translate("Eventix Importer"))
            .modal(true)
            .default_width(300)
            .default_height(120)
            .build();
        window.set_icon_name(Some(crate::APP_ID));

        // Main container
        let vbox = GtkBox::new(Orientation::Vertical, 8);
        vbox.set_margin_top(4);
        vbox.set_margin_bottom(4);
        vbox.set_margin_start(4);
        vbox.set_margin_end(4);
        window.set_child(Some(&vbox));

        // Info label
        let label = Label::new(Some(&format!(
            "<u>{}:</u>",
            locale.translate("Events/tasks to import")
        )));
        label.set_use_markup(true);
        label.set_xalign(0.0);
        vbox.append(&label);

        // load icons
        const ICON_SIZE: i32 = 25;
        let event_icon = Pixbuf::from_file_at_size(
            xdg.find_data_file("icons/event.png").unwrap(),
            ICON_SIZE,
            ICON_SIZE,
        )
        .expect("load 'icons/event.png'");
        let task_icon = Pixbuf::from_file_at_size(
            xdg.find_data_file("icons/todo.png").unwrap(),
            ICON_SIZE,
            ICON_SIZE,
        )
        .expect("load 'icons/todo.png'");

        let mut cal_filter = vec![];
        let mut type_filter = vec![];

        // build list of events/tasks to import
        let list_box = GtkBox::new(Orientation::Vertical, 12);
        for c in &model.items {
            let mut label = String::new();
            if let Some(sum) = c.summary.as_ref() {
                label.push_str(&format!("{sum}\n"));
            }
            label.push_str(&locale.date_range(
                c.start.clone(),
                c.end.clone(),
                &DateContext::system(),
                locale.timezone(),
            ));
            if let Some(rrule) = c.rrule.as_ref() {
                label.push_str(&format!("\n{}", rrule.human(locale)));
            }
            if let Some((cal_id, cal_name)) = c.exists_in.as_ref() {
                label.push_str(&format!(
                    "\n<b>Warning: UID exists in calendar '{cal_name}' and will be overwritten!</b>",
                ));
                cal_filter.push(cal_id.to_string());
            }

            let icon = match c.ty {
                CalCompType::Event => &event_icon,
                CalCompType::Todo => &task_icon,
            };
            if !type_filter.contains(&c.ty) {
                type_filter.push(c.ty);
            }

            let row = Self::create_component_row(icon, &label);
            list_box.append(&row);
        }

        vbox.append(&list_box);

        // Horizontal container for "Calendar:" + dropdown
        let calendar_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);

        let calendar_label = gtk::Label::new(Some(&format!("{}:", locale.translate("Calendar"))));
        calendar_label.set_use_markup(true);
        calendar_label.set_xalign(0.0);
        calendar_box.append(&calendar_label);

        // Build list of calendar entries (filtered), keeping ids alongside
        let filtered_cals: Vec<&ImportCalendar> = model
            .calendars
            .iter()
            .filter(|cal| {
                if !cal_filter.is_empty() && !cal_filter.contains(&cal.id) {
                    return false;
                }
                if !cal.types.iter().any(|x| type_filter.contains(x)) {
                    return false;
                }
                true
            })
            .collect();

        let (calendar_dropdown, cal_entries) =
            Self::create_color_dropdown(filtered_cals.iter().copied());
        calendar_box.append(&calendar_dropdown);

        vbox.append(&calendar_box);

        // Buttons
        let button_box = GtkBox::new(Orientation::Horizontal, 8);
        let import_button = Button::with_label(locale.translate("Import"));
        let cancel_button = Button::with_label(locale.translate("Cancel"));
        button_box.append(&import_button);
        button_box.append(&cancel_button);
        vbox.append(&button_box);

        // Connect Import
        let data = Rc::new(RefCell::new(Some(data)));
        import_button.connect_clicked(move |_| {
            let idx = calendar_dropdown.selected() as usize;
            let cal_id = cal_entries[idx].id.clone();
            let data = data.borrow_mut().take().unwrap();
            import(data, cal_id).unwrap();
            Self::quit(0);
        });

        // Connect Cancel
        cancel_button.connect_clicked(|_| Self::quit(0));

        // Quit the app when "X" is clicked
        window.connect_close_request(|_| {
            Self::quit(0);
        });

        Self { window }
    }

    pub fn show_error(message: &str) {
        let dialog = Window::builder()
            .title("Error")
            .modal(true)
            .default_width(300)
            .build();

        let vbox = GtkBox::new(Orientation::Vertical, 8);
        vbox.set_margin_top(12);
        vbox.set_margin_bottom(12);
        vbox.set_margin_start(12);
        vbox.set_margin_end(12);
        dialog.set_child(Some(&vbox));

        let label = Label::new(Some(message));
        label.set_wrap(true);
        vbox.append(&label);

        let ok_button = Button::with_label("Ok");
        ok_button.connect_clicked({
            let dialog = dialog.clone();
            move |_| {
                dialog.close();
                Self::quit(0);
            }
        });
        vbox.append(&ok_button);

        dialog.connect_close_request(|_| {
            Self::quit(0);
        });

        let main_loop = glib::MainLoop::new(None, false);
        dialog.present();
        main_loop.run();
    }

    pub fn show(&self) {
        let main_loop = glib::MainLoop::new(None, false);
        self.window.present();
        main_loop.run();
    }

    fn create_component_row(icon: &Pixbuf, text: &str) -> GtkBox {
        let row = GtkBox::new(Orientation::Horizontal, 8);

        // icon
        let image = Image::from_pixbuf(Some(icon));
        image.set_valign(Align::Start);
        row.append(&image);

        // label
        let label = Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_use_markup(true);
        label.set_valign(Align::Start);
        row.append(&label);

        row
    }

    fn create_color_dropdown<'a, I>(calendars: I) -> (DropDown, Vec<CalEntry>)
    where
        I: IntoIterator<Item = &'a ImportCalendar>,
    {
        // Build two parallel lists: display strings (for the model) and CalEntry metadata
        let mut display_strings: Vec<String> = Vec::new();
        let mut cal_entries: Vec<CalEntry> = Vec::new();

        for cal in calendars {
            let color = Self::parse_color(&cal.color).unwrap();
            let pixbuf = Self::create_color_circle(color, 16);
            display_strings.push(format!(" {}", cal.name));
            cal_entries.push(CalEntry {
                id: cal.id.clone(),
                icon: pixbuf,
            });
        }

        // Build a StringList model
        let model = gtk::StringList::new(
            &display_strings
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );

        // Build a factory that shows the color circle alongside the text
        let factory = SignalListItemFactory::new();
        let icons: Rc<Vec<Pixbuf>> = Rc::new(cal_entries.iter().map(|e| e.icon.clone()).collect());

        factory.connect_setup(|_, list_item| {
            let list_item = list_item.downcast_ref::<ListItem>().unwrap();
            let hbox = GtkBox::new(Orientation::Horizontal, 4);
            let image = Image::new();
            let label = Label::new(None);
            hbox.append(&image);
            hbox.append(&label);
            list_item.set_child(Some(&hbox));
        });

        factory.connect_bind({
            let icons = icons.clone();
            move |_, list_item| {
                let list_item = list_item.downcast_ref::<ListItem>().unwrap();
                let pos = list_item.position() as usize;
                let hbox = list_item.child().unwrap().downcast::<GtkBox>().unwrap();
                let mut children = hbox.first_child();
                let image = children
                    .as_ref()
                    .unwrap()
                    .downcast_ref::<Image>()
                    .unwrap()
                    .clone();
                children = children.unwrap().next_sibling();
                let label = children
                    .as_ref()
                    .unwrap()
                    .downcast_ref::<Label>()
                    .unwrap()
                    .clone();

                if let Some(pixbuf) = icons.get(pos) {
                    image.set_from_pixbuf(Some(pixbuf));
                }
                if let Some(item) = list_item.item() {
                    if let Some(string_obj) = item.downcast_ref::<StringObject>() {
                        label.set_text(string_obj.string().as_str());
                    }
                }
            }
        });

        let dropdown = DropDown::new(
            Some(model.upcast::<gio::ListModel>()),
            gtk::Expression::NONE,
        );
        dropdown.set_factory(Some(&factory));
        dropdown.set_selected(0);

        (dropdown, cal_entries)
    }

    fn parse_color(spec: &str) -> Option<(u8, u8, u8)> {
        let rgba = RGBA::parse(spec).ok()?;
        Some((
            (rgba.red() * 255.0).round() as u8,
            (rgba.green() * 255.0).round() as u8,
            (rgba.blue() * 255.0).round() as u8,
        ))
    }

    fn create_color_circle(color: (u8, u8, u8), size: i32) -> Pixbuf {
        let rowstride = size * 4;
        let mut data = vec![0u8; (rowstride * size) as usize];
        for y in 0..size {
            for x in 0..size {
                let dx = x - size / 2;
                let dy = y - size / 2;
                if dx * dx + dy * dy <= (size / 2) * (size / 2) {
                    let offset = (y * rowstride + x * 4) as usize;
                    data[offset] = color.0;
                    data[offset + 1] = color.1;
                    data[offset + 2] = color.2;
                    data[offset + 3] = 255;
                }
            }
        }
        Pixbuf::from_mut_slice(data, Colorspace::Rgb, true, 8, size, size, rowstride)
    }

    fn quit(code: i32) -> ! {
        std::process::exit(code);
    }
}
