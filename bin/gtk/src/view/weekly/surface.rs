// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared card ownership and interaction, independent of each surface's layout.

use std::{cell::RefCell, rc::Rc};

use eventix_view::{OccurrenceId, TimedSegmentId, WeekSnapshot};
use gtk::{glib, prelude::*};

use super::{card::EventCard, popover::DetailsController};

#[derive(PartialEq, Eq)]
pub(super) enum CardKey {
    Timed(TimedSegmentId),
    Span(OccurrenceId),
}

pub(super) struct Card {
    pub button: EventCard,
    key: CardKey,
}

#[derive(Default)]
pub(super) struct Surface {
    pub week: RefCell<Option<Rc<WeekSnapshot>>>,
    pub cards: RefCell<Vec<Card>>,
    pub details: RefCell<Rc<DetailsController>>,
    scroll_handlers: RefCell<Vec<(gtk::Adjustment, glib::SignalHandlerId)>>,
}

impl Surface {
    pub fn construct(self: &Rc<Self>, widget: &gtk::Widget) {
        let gesture = gtk::GestureClick::new();
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        gesture.connect_pressed(move |_, _, x, y| {
            let Some(surface) = weak.upgrade() else {
                return;
            };
            let on_event = surface.cards.borrow().iter().any(|card| {
                let allocation = card.button.allocation();
                x >= f64::from(allocation.x())
                    && x < f64::from(allocation.x() + allocation.width())
                    && y >= f64::from(allocation.y())
                    && y < f64::from(allocation.y() + allocation.height())
            });
            if !on_event {
                surface.details.borrow().close();
            }
        });
        widget.add_controller(gesture);
    }

    /// Reuses cards by display identity to retain keyboard focus across local-data refreshes.
    pub fn reconcile(
        self: &Rc<Self>,
        widget: &gtk::Widget,
        week: Rc<WeekSnapshot>,
        keys: Vec<CardKey>,
    ) {
        self.details.borrow().close_for(widget);
        let mut old_cards = self.cards.take();
        let mut cards = Vec::new();
        let mut previous: Option<EventCard> = None;
        for key in keys {
            let button = if let Some(index) = old_cards.iter().position(|card| card.key == key) {
                old_cards.remove(index).button
            } else {
                let button = EventCard::new();
                let parent = widget.downgrade();
                let weak = Rc::downgrade(self);
                button.connect_clicked(move |button| {
                    let (Some(parent), Some(surface)) = (parent.upgrade(), weak.upgrade()) else {
                        return;
                    };
                    if let Some(week) = surface.week.borrow().as_ref() {
                        surface
                            .details
                            .borrow()
                            .show(&parent, button, week.locale.as_ref());
                    }
                });
                button
            };
            button.insert_after(widget, previous.as_ref());
            previous = Some(button.clone());
            cards.push(Card { button, key });
        }
        for card in old_cards {
            card.button.unparent();
        }
        self.cards.replace(cards);
        self.week.replace(Some(week));
    }

    pub fn event_cards(&self) -> Vec<(i64, EventCard)> {
        self.cards
            .borrow()
            .iter()
            .filter_map(|card| {
                card.button
                    .event()
                    .map(|event| (event.start.timestamp(), card.button.clone()))
            })
            .collect()
    }

    pub fn snapshot_cards(&self, widget: &gtk::Widget, snapshot: &gtk::Snapshot) {
        for card in self.cards.borrow().iter() {
            widget.snapshot_child(&card.button, snapshot);
        }
    }

    pub fn map(&self, widget: &gtk::Widget) {
        let mut ancestor = widget.parent();
        while let Some(parent) = ancestor {
            if let Some(scroll) = parent.downcast_ref::<gtk::ScrolledWindow>() {
                for adjustment in [scroll.hadjustment(), scroll.vadjustment()] {
                    let weak = widget.downgrade();
                    let handler = adjustment.connect_value_changed(move |_| {
                        if let Some(widget) = weak.upgrade() {
                            widget.queue_allocate();
                        }
                    });
                    self.scroll_handlers
                        .borrow_mut()
                        .push((adjustment.clone(), handler));
                    let weak = widget.downgrade();
                    let handler = adjustment.connect_changed(move |_| {
                        if let Some(widget) = weak.upgrade() {
                            widget.queue_allocate();
                        }
                    });
                    self.scroll_handlers
                        .borrow_mut()
                        .push((adjustment, handler));
                }
            }
            ancestor = parent.parent();
        }
    }

    pub fn unmap(&self) {
        for (adjustment, handler) in self.scroll_handlers.take() {
            adjustment.disconnect(handler);
        }
    }

    pub fn dispose(&self, widget: &gtk::Widget) {
        self.unmap();
        self.details.borrow().close_for(widget);
        for card in self.cards.take() {
            card.button.unparent();
        }
        // Closed popovers can still be parented while their idle cleanup is pending.
        while let Some(child) = widget.first_child() {
            child.unparent();
        }
    }
}
