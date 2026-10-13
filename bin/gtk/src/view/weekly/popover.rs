// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use eventix_locale::Locale;
use gtk::{glib, graphene, prelude::*};

use super::card::EventCard;
use crate::view::details::EventDetails;

const PLACEMENT_MARGIN: i32 = 16;
const PLACEMENT_HYSTERESIS: i32 = 64;

/// Owns the single details popup shared by the week's event surfaces.
#[derive(Default)]
pub(crate) struct DetailsController {
    popover: RefCell<Option<EventDetails>>,
    parent: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    anchor: RefCell<Option<glib::WeakRef<EventCard>>>,
    minimum_height: Cell<i32>,
    width: Cell<i32>,
    centered: Cell<bool>,
    force_centered: Cell<bool>,
    rejected: Cell<bool>,
}

impl DetailsController {
    pub fn close(&self) {
        self.anchor.take();
        self.parent.take();
        if let Some(popover) = self.popover.take() {
            popover.popdown();
            Self::defer_unparent(&popover);
        }
    }

    pub fn close_for(&self, parent: &gtk::Widget) {
        if self.is_parent(parent) {
            self.close();
        }
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
        if self.is_parent(parent) {
            let popover = self.popover.borrow().clone();
            if let Some(popover) = popover {
                self.place(&popover);
                popover.present();
            }
        }
    }

    pub fn show(self: &Rc<Self>, parent: &gtk::Widget, button: &EventCard, locale: &dyn Locale) {
        self.close();
        let Some(event) = button.event() else { return };
        let popover = EventDetails::new(&event, locale);
        popover.set_parent(parent);
        // Measure once before changing arrow and width requests for centered placement.
        let (width, minimum_height) = popover.placement_size();
        self.width.set(width);
        self.minimum_height.set(minimum_height);
        self.centered.set(false);
        self.force_centered.set(false);
        self.rejected.set(false);
        self.parent.replace(Some(parent.downgrade()));
        self.anchor.replace(Some(button.downgrade()));
        self.place(&popover);
        let weak = Rc::downgrade(self);
        popover.connect_closed(move |closed| {
            let weak = weak.clone();
            let closed = closed.clone();
            glib::idle_add_local_once(move || {
                if let Some(controller) = weak.upgrade()
                    && controller.popover.borrow().as_ref() == Some(&closed)
                {
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
                    controller.anchor.take();
                    controller.parent.take();
                }
                if closed.parent().is_some() {
                    closed.unparent();
                }
            });
        });
        self.popover.replace(Some(popover.clone()));
        popover.popup();
        if let Some(surface) = popover.surface() {
            let weak = Rc::downgrade(self);
            let popup = popover.downgrade();
            surface.connect_layout(move |_, _, height| {
                if let (Some(controller), Some(popup)) = (weak.upgrade(), popup.upgrade())
                    && controller.popover.borrow().as_ref() == Some(&popup)
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
        let Some(button) = self
            .anchor
            .borrow()
            .as_ref()
            .and_then(|anchor| anchor.upgrade())
        else {
            return;
        };
        let Some(root) = parent.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let bounds = button.compute_bounds(&root);
        // Only the intersection of all enclosing viewports is a usable popup anchor.
        let mut visible = Some(graphene::Rect::new(
            0.0,
            0.0,
            root.width() as f32,
            root.height() as f32,
        ));
        let mut ancestor = parent.parent();
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
        let available = visible_anchor.map_or(0, |bounds| {
            (bounds.y() as i32).max(root.height() - (bounds.y() + bounds.height()).ceil() as i32)
        });
        let centered = visible_anchor.is_none()
            || self.force_centered.get()
            || details_should_center(available, self.minimum_height.get(), self.centered.get());
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
            popover.set_position(gtk::PositionType::Bottom);
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
    use super::details_should_center;

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
