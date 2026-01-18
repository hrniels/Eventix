use eventix_ical::objects::CalCompType;
use eventix_locale::Locale;
use gdk_pixbuf::{Colorspace, Pixbuf};
use glib::value::ToValue;
use gtk::gdk::RGBA;
use gtk::gio::Icon;
use gtk::{Align, ButtonsType, DialogFlags, Image, MessageDialog, MessageType, prelude::*};
use gtk::{
    Box as GtkBox, Button, CellRendererPixbuf, CellRendererText, ComboBox, Dialog, Label,
    ListStore, Orientation,
};
use std::cell::RefCell;
use std::rc::Rc;
use xdg::BaseDirectories;

use crate::model::{ImportCalendar, ImportModel};

pub struct ImportView {
    dialog: Dialog,
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
        // Create the dialog (no parent window needed in GTK3)
        let dialog = Dialog::new();
        dialog.set_title(locale.translate("Eventix Importer"));
        dialog.set_modal(true);
        dialog.set_default_size(300, 120);
        dialog.set_icon_name(Some(crate::APP_ID));

        // Main container
        let content_area = dialog.content_area();
        let vbox = GtkBox::new(Orientation::Vertical, 8);
        vbox.set_margin_top(4);
        vbox.set_margin_bottom(4);
        vbox.set_margin_start(4);
        vbox.set_margin_end(4);
        content_area.pack_start(&vbox, true, true, 0);

        // Info label
        let label = Label::new(Some(&format!(
            "<u>{}:</u>",
            locale.translate("Events/tasks to import")
        )));
        label.set_use_markup(true);
        label.set_xalign(0.0);
        vbox.pack_start(&label, false, false, 0);

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
            label.push_str(&locale.date_range(c.start.clone(), c.end.clone()));
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
            list_box.pack_start(&row, false, false, 0);
        }

        vbox.pack_start(&list_box, false, false, 0);

        // Horizontal container for "Calendar:" + combobox
        let calendar_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);

        let calendar_label = gtk::Label::new(Some(&format!("{}:", locale.translate("Calendar"))));
        calendar_label.set_use_markup(true);
        calendar_label.set_xalign(0.0); // left-align the label
        calendar_box.pack_start(&calendar_label, false, false, 0);

        let calendar_combo = Self::create_color_combo(model.calendars.iter().filter(|cal| {
            // Filter out categories that we can't import into
            if !cal_filter.is_empty() && !cal_filter.contains(&cal.id) {
                return false;
            }
            if !cal.types.iter().any(|x| type_filter.contains(x)) {
                return false;
            }
            true
        }));
        calendar_box.pack_start(&calendar_combo, true, true, 0);

        vbox.pack_start(&calendar_box, false, false, 0);

        // Buttons
        let button_box = GtkBox::new(Orientation::Horizontal, 8);
        let import_button = Button::with_label(locale.translate("Import"));
        let cancel_button = Button::with_label(locale.translate("Cancel"));
        button_box.pack_start(&import_button, true, true, 0);
        button_box.pack_start(&cancel_button, true, true, 0);
        vbox.pack_start(&button_box, false, false, 0);

        // Connect Import
        let calendar_combo_clone = calendar_combo.clone();
        let data = Rc::new(RefCell::new(Some(data)));
        import_button.connect_clicked(move |_| {
            let iter = calendar_combo_clone.active_iter().unwrap();
            let cb_model = calendar_combo_clone.model().unwrap();
            let cal_id: String = cb_model.value(&iter, 2).get().unwrap();
            let data = data.borrow_mut().take().unwrap();
            import(data, cal_id).unwrap();
            Self::quit(0);
        });

        // Connect Cancel
        cancel_button.connect_clicked(|_| Self::quit(0));

        // Quit the app when "X" is clicked
        dialog.connect_delete_event(|_, _| Self::quit(0));

        Self { dialog }
    }

    pub fn show_error(message: &str) {
        let dialog = MessageDialog::new::<gtk::Window>(
            None,
            DialogFlags::MODAL,
            MessageType::Error,
            ButtonsType::Ok,
            message,
        );
        dialog.run();
        dialog.close();
    }

    pub fn show(&self) {
        self.dialog.show_all();
        gtk::main();
    }

    fn create_component_row(icon: &Pixbuf, text: &str) -> GtkBox {
        let row = GtkBox::new(Orientation::Horizontal, 8);

        // icon
        let image = Image::from_pixbuf(Some(icon));
        image.set_valign(Align::Start);
        row.pack_start(&image, false, false, 0);

        // label
        let label = Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_use_markup(true);
        label.set_valign(Align::Start);
        row.pack_start(&label, true, true, 0);

        row
    }

    fn create_color_combo<'a, I>(calendars: I) -> ComboBox
    where
        I: Iterator<Item = &'a ImportCalendar>,
    {
        let store = ListStore::new(&[
            Icon::static_type(),
            String::static_type(),
            String::static_type(),
        ]);

        for cal in calendars {
            let color = Self::parse_color(&cal.color).unwrap();
            let name = format!(" {}", cal.name);
            let pixbuf = Self::create_color_circle(color, 16);
            store.insert_with_values(None, &[(0, &pixbuf.to_value()), (1, &name), (2, &cal.id)]);
        }

        let combo = ComboBox::with_model(&store);

        // Cell renderer for the icon
        let pixbuf_renderer = CellRendererPixbuf::new();
        combo.pack_start(&pixbuf_renderer, false);
        combo.add_attribute(&pixbuf_renderer, "pixbuf", 0);

        // Cell renderer for the text
        let text_renderer = CellRendererText::new();
        combo.pack_start(&text_renderer, true);
        combo.add_attribute(&text_renderer, "text", 1);

        combo.set_active(Some(0));
        combo
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
        gtk::main_quit();
        std::process::exit(code);
    }
}
