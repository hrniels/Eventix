// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use eventix_locale::Locale;
use eventix_view::sidebar::SidebarSnapshot;
use gtk::{cairo, gdk::prelude::GdkCairoContextExt, gdk_pixbuf::Pixbuf, glib, prelude::*};
use ksni::{Icon, ToolTip, blocking::TrayMethods, menu::StandardItem};

use crate::window::Window;

enum Message {
    Action(&'static str),
    Toggle,
    Quit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TaskStatus {
    #[default]
    None,
    DueToday(usize),
    Overdue(usize),
}

impl TaskStatus {
    fn from_counts(today: usize, overdue: usize) -> Self {
        if overdue > 0 {
            Self::Overdue(overdue)
        } else if today > 0 {
            Self::DueToday(today)
        } else {
            Self::None
        }
    }
}

pub struct Tray {
    handle: ksni::blocking::Handle<EventixTray>,
    receiver: glib::JoinHandle<()>,
}

impl Tray {
    pub fn new(window: &Window, locale: Arc<dyn Locale + Send + Sync>) -> anyhow::Result<Self> {
        let (sender, receiver) = async_channel::unbounded();
        let tray = EventixTray {
            sender,
            locale,
            status: TaskStatus::None,
            icon: render_icon(TaskStatus::None)?,
        };
        let handle = tray.disable_dbus_name(true).spawn()?;
        let weak = window.downgrade();
        let receiver = glib::MainContext::default().spawn_local(async move {
            while let Ok(message) = receiver.recv().await {
                let Some(window) = weak.upgrade() else {
                    break;
                };
                match message {
                    Message::Action(action) => {
                        window.present_from_tray();
                        gtk::prelude::ActionGroupExt::activate_action(&window, action, None);
                    }
                    Message::Toggle => {
                        if window.is_visible() {
                            window.hide_to_tray();
                        } else {
                            window.present_from_tray();
                        }
                    }
                    Message::Quit => {
                        if let Some(app) = window.application() {
                            app.quit();
                        }
                    }
                }
            }
        });
        Ok(Self { handle, receiver })
    }

    pub fn update(&self, sidebar: &SidebarSnapshot) {
        let rows = || sidebar.tasks.iter().flat_map(|group| &group.rows);
        let status = TaskStatus::from_counts(
            rows().filter(|row| row.due_today).count(),
            rows().filter(|row| row.overdue).count(),
        );
        self.handle.update(|tray| {
            tray.locale = sidebar.locale.clone();
            if tray.status != status {
                match render_icon(status) {
                    Ok(icon) => {
                        tray.icon = icon;
                        tray.status = status;
                    }
                    Err(error) => tracing::warn!(%error, "Rendering tray icon failed"),
                }
            }
        });
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.receiver.abort();
        self.handle.shutdown();
    }
}

struct EventixTray {
    sender: async_channel::Sender<Message>,
    locale: Arc<dyn Locale + Send + Sync>,
    status: TaskStatus,
    icon: Icon,
}

impl EventixTray {
    fn send(&self, message: Message) {
        if let Err(error) = self.sender.try_send(message) {
            tracing::warn!(%error, "Sending tray action failed");
        }
    }
}

impl ksni::Tray for EventixTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        "Eventix".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![self.icon.clone()]
    }

    fn attention_icon_pixmap(&self) -> Vec<Icon> {
        self.icon_pixmap()
    }

    fn tool_tip(&self) -> ToolTip {
        let title = match self.status {
            TaskStatus::None => self.locale.translate("No tasks due today").to_owned(),
            TaskStatus::DueToday(count) => {
                formatx::formatx!(self.locale.translate("{} task(s) due today"), count).unwrap()
            }
            TaskStatus::Overdue(count) => {
                formatx::formatx!(self.locale.translate("{} overdue task(s)!"), count).unwrap()
            }
        };
        ToolTip {
            title,
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut menu = Vec::new();
        for (label, icon_data, action) in [
            (
                "Monthly",
                include_bytes!("../../../data/icons/month.png").as_slice(),
                "month-view",
            ),
            (
                "Weekly",
                include_bytes!("../../../data/icons/week.png").as_slice(),
                "week-view",
            ),
            (
                "List",
                include_bytes!("../../../data/icons/list.png").as_slice(),
                "list-view",
            ),
            (
                "New Event",
                include_bytes!("../../../data/icons/event.png").as_slice(),
                "new-event",
            ),
            (
                "New Task",
                include_bytes!("../../../data/icons/todo.png").as_slice(),
                "new-task",
            ),
        ] {
            if label == "New Event" {
                menu.push(ksni::MenuItem::Separator);
            }
            menu.push(
                StandardItem {
                    label: self.locale.translate(label).to_owned(),
                    icon_data: icon_data.to_vec(),
                    activate: Box::new(move |tray: &mut Self| tray.send(Message::Action(action))),
                    ..Default::default()
                }
                .into(),
            );
        }
        menu.push(ksni::MenuItem::Separator);
        menu.push(
            StandardItem {
                label: self.locale.translate("Exit").to_owned(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Message::Quit)),
                ..Default::default()
            }
            .into(),
        );
        menu
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Message::Toggle);
    }
}

/// Renders the task badge and converts Cairo's native premultiplied pixels to SNI ARGB.
fn render_icon(status: TaskStatus) -> anyhow::Result<Icon> {
    let bytes = glib::Bytes::from_static(include_bytes!("../../../data/static/icon.png"));
    let stream = gtk::gio::MemoryInputStream::from_bytes(&bytes);
    let pixbuf = Pixbuf::from_stream(&stream, gtk::gio::Cancellable::NONE)?;
    let (width, height) = (pixbuf.width(), pixbuf.height());
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height)?;
    let cr = cairo::Context::new(&surface)?;
    cr.set_source_pixbuf(&pixbuf, 0.0, 0.0);
    cr.paint()?;
    if let TaskStatus::DueToday(count) | TaskStatus::Overdue(count) = status {
        let radius = f64::from(width) / 3.3;
        let cx = f64::from(width) - radius - 4.0;
        let cy = f64::from(height) - radius - 4.0;
        cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
        cr.set_source_rgb(
            if matches!(status, TaskStatus::Overdue(_)) {
                1.0
            } else {
                0.0
            },
            0.0,
            0.0,
        );
        cr.fill()?;
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(f64::from(width) / 1.8);
        let text = if count >= 10 {
            "+".into()
        } else {
            count.to_string()
        };
        let extents = cr.text_extents(&text)?;
        cr.move_to(
            cx - extents.width() / 2.0 - extents.x_bearing(),
            cy - extents.height() / 2.0 - extents.y_bearing(),
        );
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.show_text(&text)?;
    }
    drop(cr);
    let stride = surface.stride() as usize;
    let pixels = surface.take_data()?;
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for row in pixels.chunks(stride).take(height as usize) {
        for pixel in row[..width as usize * 4].as_chunks::<4>().0 {
            let argb = u32::from_ne_bytes(*pixel);
            let a = (argb >> 24) as u8;
            let straight = |shift: u32| {
                let value = (argb >> shift) & 255;
                if a == 0 {
                    0
                } else {
                    ((value * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
                }
            };
            data.extend_from_slice(&[a, straight(16), straight(8), straight(0)]);
        }
    }
    Ok(Icon {
        width,
        height,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overdue_tasks_take_priority() {
        assert_eq!(TaskStatus::from_counts(0, 0), TaskStatus::None);
        assert_eq!(TaskStatus::from_counts(3, 0), TaskStatus::DueToday(3));
        assert_eq!(TaskStatus::from_counts(3, 2), TaskStatus::Overdue(2));
    }

    #[test]
    fn renders_badges_in_argb_format() {
        let base = render_icon(TaskStatus::None).unwrap();
        let today = render_icon(TaskStatus::DueToday(2)).unwrap();
        let overdue = render_icon(TaskStatus::Overdue(2)).unwrap();
        assert_eq!(base.data.len(), (base.width * base.height * 4) as usize);
        assert_ne!(base.data, today.data);
        assert_ne!(today.data, overdue.data);
        assert_eq!(
            render_icon(TaskStatus::DueToday(10)).unwrap().data,
            render_icon(TaskStatus::DueToday(20)).unwrap().data
        );
    }
}
