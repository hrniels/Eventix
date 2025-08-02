use std::path::PathBuf;

use async_channel::Sender;
use gdk_pixbuf::Pixbuf;
use gtk::cairo::{self, Format, ImageSurface};
use ksni::{Icon, Status, ToolTip};

pub enum TrayMessage {
    LoadPage(String),
    ToggleWindow,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    None,
    DueToday(u32),
    Overdue(u32),
}

pub struct EventixTray {
    theme_path: PathBuf,
    sender: Sender<TrayMessage>,
    status: TaskStatus,
    icon: Vec<u8>,
    width: i32,
    height: i32,
}

impl EventixTray {
    pub fn new(sender: Sender<TrayMessage>, theme_path: PathBuf, path: PathBuf) -> Self {
        let icon = Pixbuf::from_file(&path).unwrap_or_else(|_| panic!("load icon '{path:?}'"));
        EventixTray {
            theme_path,
            sender,
            icon: icon.pixel_bytes().unwrap().to_vec(),
            width: icon.width(),
            height: icon.height(),
            status: TaskStatus::None,
        }
    }

    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
    }

    pub fn load_page(&self, uri: &str) {
        if let Err(e) = self.sender.try_send(TrayMessage::LoadPage(uri.into())) {
            eprintln!("Failed to send message: {e:?}");
        }
    }

    fn create_pixbuf(&self) -> Pixbuf {
        let width = self.width;
        let height = self.height;

        // Create Cairo surface
        let surface =
            cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).expect("surface");
        let cr = cairo::Context::new(&surface).unwrap();

        // Draw base icon
        let data = self.icon.as_slice();
        let base_surface = ImageSurface::create_for_data(
            data.to_vec(),
            Format::ARgb32,
            self.width,
            self.height,
            self.width * 4,
        )
        .unwrap();
        cr.set_source_surface(&base_surface, 0.0, 0.0).unwrap();
        cr.paint().unwrap();

        let due = match self.status {
            TaskStatus::DueToday(count) => Some(count),
            TaskStatus::Overdue(count) => Some(count),
            _ => None,
        };

        if let Some(due) = due {
            // Draw the circle
            let radius = width as f64 / 3.3;
            let cx = width as f64 - radius - 4.0;
            let cy = height as f64 - radius - 4.0;
            cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
            match self.status {
                // red for overdue
                TaskStatus::Overdue(_) => cr.set_source_rgba(0.0, 0.0, 1.0, 1.0),
                // black for due today
                _ => cr.set_source_rgba(0.0, 0.0, 0.0, 1.0),
            }
            cr.fill().unwrap();

            // Draw the text
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(width as f64 / 1.8);
            let text = if due >= 10 {
                String::from("+")
            } else {
                format!("{due}")
            };
            let te = cr.text_extents(&text).unwrap();
            cr.move_to(
                cx - te.width() / 2.0 - te.x_bearing(),
                cy - te.height() / 2.0 - te.y_bearing(),
            );
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.show_text(&text).unwrap();
        }

        drop(cr);

        // Convert the Cairo surface to Pixbuf
        let stride = surface.stride();
        let data = surface.take_data().unwrap();
        Pixbuf::from_mut_slice(
            data,
            gdk_pixbuf::Colorspace::Rgb,
            true,
            8,
            self.width,
            self.height,
            stride,
        )
    }

    fn create_icon(&self) -> Icon {
        let pixbuf = self.create_pixbuf();

        let width = pixbuf.width();
        let height = pixbuf.height();
        let pixels = pixbuf.pixel_bytes().unwrap();
        let stride = pixbuf.rowstride() as usize;

        // ksni icons expect ARGB format
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let row = &pixels[(y as usize) * stride..(y as usize) * stride + width as usize * 4];
            for px in row.chunks_exact(4) {
                let r = px[0];
                let g = px[1];
                let b = px[2];
                let a = px[3];
                data.extend_from_slice(&[a, r, g, b]);
            }
        }

        Icon {
            width: self.width,
            height: self.height,
            data,
        }
    }
}

impl ksni::Tray for EventixTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn icon_theme_path(&self) -> String {
        self.theme_path.to_str().unwrap().to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![self.create_icon()]
    }

    fn attention_icon_pixmap(&self) -> Vec<Icon> {
        vec![self.create_icon()]
    }

    fn status(&self) -> Status {
        Status::Active
    }

    fn tool_tip(&self) -> ToolTip {
        let mut tt = ToolTip::default();
        match self.status {
            TaskStatus::None => {
                tt.title = "No tasks due today".to_string();
            }
            TaskStatus::DueToday(count) => {
                tt.title = format!("{count} task(s) due today");
            }
            TaskStatus::Overdue(count) => {
                tt.title = format!("{count} overdue task(s)!");
            }
        }
        tt
    }

    fn title(&self) -> String {
        "Eventix".into()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Monthly".into(),
                icon_name: "static/month".into(),
                activate: Box::new(|tray: &mut EventixTray| {
                    tray.load_page("/");
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Weekly".into(),
                icon_name: "static/week".into(),
                activate: Box::new(|tray: &mut EventixTray| {
                    tray.load_page("/weekly");
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "List".into(),
                icon_name: "static/list".into(),
                activate: Box::new(|tray: &mut EventixTray| {
                    tray.load_page("/list");
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "New Event".into(),
                icon_name: "static/event".into(),
                activate: Box::new(|tray: &mut EventixTray| {
                    tray.load_page("/new?ctype=Event");
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "New Task".into(),
                icon_name: "static/todo".into(),
                activate: Box::new(|tray: &mut EventixTray| {
                    tray.load_page("/new?ctype=Todo");
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Exit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        if let Err(e) = self.sender.try_send(TrayMessage::ToggleWindow) {
            eprintln!("Failed to send message: {e:?}");
        }
    }
}
