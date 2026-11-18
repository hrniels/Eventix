// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::prelude::*;

/// Traverses an explicit tab order without wrapping or trapping focus in the calendar.
pub(crate) fn traverse(container: &gtk::Widget, order: &[gtk::Widget], forward: bool) -> bool {
    let current = container
        .root()
        .and_then(|root| root.focus())
        .and_then(|focus| order.iter().position(|widget| *widget == focus));
    focus_index(order.len(), current, forward).is_some_and(|index| grab_focus(&order[index]))
}

pub(crate) fn focus_index(count: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    match current {
        None if forward => (count > 0).then_some(0),
        None => count.checked_sub(1),
        Some(index) if forward => index.checked_add(1).filter(|next| *next < count),
        Some(index) => index.checked_sub(1),
    }
}

/// Focuses a calendar control and reveals it in each enclosing scroll viewport.
pub(crate) fn grab_focus(widget: &gtk::Widget) -> bool {
    if !widget.grab_focus() {
        return false;
    }
    if let Some(window) = widget.root().and_downcast::<gtk::Window>() {
        window.set_focus_visible(true);
    }

    let mut ancestor = widget.parent();
    while let Some(parent) = ancestor {
        if let Some(scroll) = parent.downcast_ref::<gtk::ScrolledWindow>()
            && let Some(child) = scroll.child()
        {
            let content = child
                .downcast_ref::<gtk::Viewport>()
                .and_then(|viewport| viewport.child())
                .unwrap_or(child);
            if let Some(bounds) = widget.compute_bounds(&content) {
                scroll.vadjustment().clamp_page(
                    f64::from(bounds.y()),
                    f64::from(bounds.y() + bounds.height()),
                );
                scroll.hadjustment().clamp_page(
                    f64::from(bounds.x()),
                    f64::from(bounds.x() + bounds.width()),
                );
            }
        }
        ancestor = parent.parent();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::focus_index;

    #[test]
    fn tab_order_stops_at_boundaries() {
        assert_eq!(focus_index(3, None, true), Some(0));
        assert_eq!(focus_index(3, None, false), Some(2));
        assert_eq!(focus_index(3, Some(0), true), Some(1));
        assert_eq!(focus_index(3, Some(2), false), Some(1));
        assert_eq!(focus_index(3, Some(2), true), None);
        assert_eq!(focus_index(3, Some(0), false), None);
        assert_eq!(focus_index(0, None, true), None);
        assert_eq!(focus_index(0, None, false), None);
    }
}
