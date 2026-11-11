// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::Locale;
use gtk::prelude::*;

struct Section {
    name: &'static str,
    title: &'static str,
    shortcuts: &'static [(&'static str, &'static str)],
    mouse: &'static [(&'static str, &'static str)],
}

const SECTIONS: &[Section] = &[
    Section {
        name: "general",
        title: "General",
        shortcuts: &[
            ("<Primary>question", "Keyboard Shortcuts"),
            ("<Primary>f", "Search"),
            ("<Control>e", "New event"),
            ("<Control>t", "New task"),
            ("<Control>a", "Focus quick task entry"),
            ("<Primary>r F5", "Synchronize all calendars"),
            ("<Control>w", "Weekly View"),
            ("<Control>m", "Monthly View"),
            ("<Control>l", "List View"),
            ("<Control>Home <Control>period", "Today"),
            ("Escape", "Close popup or cancel interaction"),
        ],
        mouse: &[],
    },
    Section {
        name: "weekly",
        title: "Weekly View",
        shortcuts: &[
            ("<Control>Left", "Previous week"),
            ("<Control>Right", "Next week"),
            ("Left Up", "Focus previous event"),
            ("Right Down", "Focus next event"),
            ("Return space", "Open focused event"),
        ],
        mouse: &[
            ("Open event details", "Click an event."),
            ("Open event actions", "Right-click an event."),
            ("New event", "Double-click an empty time slot."),
            (
                "New all-day event",
                "Double-click a day heading or empty all-day area.",
            ),
            (
                "Create event for selected times",
                "Drag across empty time slots.",
            ),
            (
                "Create event for selected days",
                "Drag across empty all-day cells.",
            ),
            (
                "Move event",
                "Drag an editable event to another time or day.",
            ),
            (
                "Copy event",
                "Hold Ctrl while dragging a non-recurring editable event.",
            ),
            (
                "Resize event",
                "Drag the top or bottom edge of an editable timed event.",
            ),
        ],
    },
    Section {
        name: "monthly",
        title: "Monthly View",
        shortcuts: &[
            ("<Control>Left", "Previous month"),
            ("<Control>Right", "Next month"),
            ("Left Up", "Focus previous event"),
            ("Right Down", "Focus next event"),
            ("Return space", "Open focused event"),
        ],
        mouse: &[
            ("Open event details", "Click an event."),
            ("Open event actions", "Right-click an event."),
            ("New all-day event", "Double-click an empty day cell."),
            ("Open week", "Click a week number."),
            (
                "Create event for selected days",
                "Drag across empty day cells.",
            ),
            (
                "Move event",
                "Drag an editable event to another day; its time is preserved.",
            ),
            (
                "Copy event",
                "Hold Ctrl while dragging a non-recurring editable event.",
            ),
        ],
    },
    Section {
        name: "list",
        title: "List View",
        shortcuts: &[
            ("Up Down", "Move between results"),
            ("Page_Up Page_Down", "Move by one page"),
            ("Home End", "First or last result"),
            ("Return", "Open selected result"),
            ("space", "Toggle occurrences"),
            ("Right", "Expand occurrences"),
            ("Left", "Collapse occurrences"),
            ("Down", "Focus results from search filters"),
        ],
        mouse: &[("Toggle occurrences", "Click a recurring item row.")],
    },
    Section {
        name: "editing",
        title: "Editing",
        shortcuts: &[
            ("<Control>Return", "Save event or task"),
            ("Escape", "Close popup or cancel editing"),
            ("Up Down", "Select completion suggestion"),
            ("Tab Return", "Accept completion suggestion"),
        ],
        mouse: &[],
    },
    Section {
        name: "date",
        title: "Date picker and date fields",
        shortcuts: &[
            ("Left Right", "Previous or next day (week in week picker)"),
            ("Up Down", "Previous or next week"),
            ("Page_Up Page_Down", "Previous or next month"),
            (
                "<Control>Page_Up <Control>Page_Down",
                "Previous or next year",
            ),
            ("<Control>Left <Control>Right", "Adjust date by one day"),
            ("<Control>Up <Control>Down", "Adjust date by one week"),
            ("<Alt>Left <Alt>Right", "Previous or next month"),
            ("<Alt>Down <Alt>Up", "Previous or next year"),
            ("Return space", "Confirm picker selection"),
            ("Escape", "Close picker"),
        ],
        mouse: &[
            ("Confirm picker selection", "Click a day or week."),
            ("Previous or next month", "Scroll over the date picker."),
        ],
    },
    Section {
        name: "month",
        title: "Month picker",
        shortcuts: &[
            ("Left Right", "Previous or next month"),
            ("<Control>Left <Control>Right", "Previous or next month"),
            ("Page_Up Page_Down", "Previous or next year"),
            ("<Control>Up <Control>Down", "Previous or next year"),
            ("Up Down", "Move between month rows"),
            ("Return space", "Confirm picker selection"),
            ("Escape", "Close picker"),
        ],
        mouse: &[
            ("Confirm picker selection", "Click a month."),
            ("Previous or next year", "Scroll over the month picker."),
        ],
    },
    Section {
        name: "time",
        title: "Time fields",
        shortcuts: &[
            ("<Control>Left", "Decrease minutes by five"),
            ("<Control>Right", "Increase minutes by five"),
            ("<Control>Up", "Decrease hours by one"),
            ("<Control>Down", "Increase hours by one"),
            ("Escape", "Close picker"),
        ],
        mouse: &[
            (
                "Select time",
                "Click an hour, then a minute on the clock face.",
            ),
            (
                "Select precise minutes",
                "Drag on the minute clock face; release to confirm.",
            ),
        ],
    },
];

struct HelpRow {
    widget: gtk::Box,
    keywords: String,
}

struct HelpGroup {
    widget: gtk::Box,
    rows: Vec<HelpRow>,
}

struct HelpSection {
    widget: gtk::Box,
    groups: Vec<HelpGroup>,
}

/// Builds a searchable dialog with all keyboard and mouse sections in one scrolling page.
pub(crate) fn build(locale: &dyn Locale) -> gtk::Window {
    let window = gtk::Window::builder()
        .title(locale.translate("Keyboard Shortcuts"))
        .modal(true)
        .hide_on_close(true)
        .destroy_with_parent(true)
        .default_width(760)
        .default_height(720)
        .build();
    window.add_css_class("ev_shortcuts_dialog");

    let search = gtk::SearchEntry::builder()
        .placeholder_text(locale.translate("Search shortcuts"))
        .hexpand(true)
        .build();
    search.update_property(&[gtk::accessible::Property::Label(
        locale.translate("Search shortcuts"),
    )]);
    let header = gtk::HeaderBar::new();
    header.set_title_widget(Some(&search));
    window.set_titlebar(Some(&header));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 28);
    content.add_css_class("ev_shortcuts_content");
    let sections: Vec<_> = SECTIONS
        .iter()
        .map(|section| build_section(section, locale))
        .collect();
    for section in &sections {
        content.append(&section.widget);
    }
    let empty = gtk::Label::new(Some(locale.translate("No shortcuts found")));
    empty.set_visible(false);
    content.append(&empty);

    let scroll = gtk::ScrolledWindow::builder()
        .focusable(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&content)
        .build();
    window.set_child(Some(&scroll));
    let adjustment = scroll.vadjustment();
    search.connect_changed(move |search| {
        filter_sections(&sections, search.text().as_str(), &empty);
        adjustment.set_value(adjustment.lower());
    });

    let entry = search.clone();
    let adjustment = scroll.vadjustment();
    window.connect_map(move |_| {
        entry.set_text("");
        entry.grab_focus();
        adjustment.set_value(adjustment.lower());
    });
    connect_keys(&window, &search);
    window
}

fn build_section(section: &Section, locale: &dyn Locale) -> HelpSection {
    let widget = gtk::Box::new(gtk::Orientation::Vertical, 12);
    widget.set_widget_name(section.name);
    widget.add_css_class("ev_shortcuts_section");
    let heading = gtk::Label::builder()
        .label(locale.translate(section.title))
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    heading.set_xalign(0.0);
    heading.add_css_class("ev_shortcuts_heading");
    widget.append(&heading);

    let mut groups = Vec::new();
    for mouse in [false, true] {
        let entries = if mouse {
            section.mouse
        } else {
            section.shortcuts
        };
        if entries.is_empty() {
            continue;
        }
        let group = gtk::Box::new(gtk::Orientation::Vertical, 8);
        if mouse {
            let heading = gtk::Label::new(Some(locale.translate("Mouse interactions")));
            heading.set_xalign(0.0);
            heading.add_css_class("ev_shortcuts_subheading");
            group.append(&heading);
        }
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("ev_shortcuts_card");
        card.set_overflow(gtk::Overflow::Hidden);
        let rows = entries
            .iter()
            .map(|(first, second)| {
                let row = build_row(section, first, second, mouse, locale);
                card.append(&row.widget);
                row
            })
            .collect();
        group.append(&card);
        widget.append(&group);
        groups.push(HelpGroup {
            widget: group,
            rows,
        });
    }
    HelpSection { widget, groups }
}

fn build_row(
    section: &Section,
    first: &str,
    second: &str,
    mouse: bool,
    locale: &dyn Locale,
) -> HelpRow {
    let widget = gtk::Box::new(gtk::Orientation::Horizontal, 20);
    widget.add_css_class("ev_shortcuts_row");
    let title = if mouse { first } else { second };
    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some(locale.translate(title)));
    label.set_xalign(0.0);
    label.set_wrap(true);
    text.append(&label);
    if mouse {
        let subtitle = gtk::Label::new(Some(locale.translate(second)));
        subtitle.set_xalign(0.0);
        subtitle.set_wrap(true);
        subtitle.add_css_class("ev_shortcuts_description");
        text.append(&subtitle);
    }
    widget.append(&text);

    if mouse {
        let icon = gtk::Image::from_icon_name("input-mouse-symbolic");
        icon.set_valign(gtk::Align::Center);
        widget.append(&icon);
    } else {
        let keys = gtk::ShortcutLabel::new(first);
        keys.set_valign(gtk::Align::Center);
        keys.set_halign(gtk::Align::End);
        widget.append(&keys);
    }

    let keywords = format!(
        "{} {} {title} {} {}",
        locale.translate(section.title),
        section.title,
        locale.translate(title),
        if mouse {
            locale.translate(second)
        } else {
            first
        },
    )
    .to_lowercase();
    HelpRow { widget, keywords }
}

fn filter_sections(sections: &[HelpSection], query: &str, empty: &gtk::Label) {
    let query = query.to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    let mut any_visible = false;
    for section in sections {
        let mut section_visible = false;
        for group in &section.groups {
            let mut group_visible = false;
            for row in &group.rows {
                let visible = terms.iter().all(|term| row.keywords.contains(term));
                row.widget.set_visible(visible);
                group_visible |= visible;
            }
            group.widget.set_visible(group_visible);
            section_visible |= group_visible;
        }
        section.widget.set_visible(section_visible);
        any_visible |= section_visible;
    }
    empty.set_visible(!any_visible);
}

fn connect_keys(window: &gtk::Window, search: &gtk::SearchEntry) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = window.downgrade();
    let entry = search.clone();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if key == gtk::gdk::Key::Escape {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
            return gtk::glib::Propagation::Stop;
        }
        if key == gtk::gdk::Key::f && modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            entry.grab_focus();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    window.add_controller(keys);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_help_labels_have_english_and_german_translations() {
        let directory = tempfile::tempdir().unwrap();
        let xdg = xdg::BaseDirectories::with_prefix(directory.path().file_name().unwrap());
        for language in [
            eventix_locale::LocaleType::English,
            eventix_locale::LocaleType::German,
        ] {
            let locale = eventix_locale::new_with_timezone(&xdg, language, chrono_tz::UTC).unwrap();
            for section in SECTIONS {
                for label in std::iter::once(section.title)
                    .chain(section.shortcuts.iter().map(|(_, label)| *label))
                    .chain(
                        section
                            .mouse
                            .iter()
                            .flat_map(|(title, subtitle)| [*title, *subtitle]),
                    )
                    .chain(std::iter::once("Mouse interactions"))
                    .chain(["Search shortcuts", "No shortcuts found"])
                {
                    assert!(
                        locale.translations().table.contains_key(label),
                        "missing {language:?} translation: {label}"
                    );
                }
            }
        }
    }

    fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut widgets = Vec::new();
        let mut child = widget.first_child();
        while let Some(current) = child {
            widgets.extend(descendants(&current));
            child = current.next_sibling();
            widgets.push(current);
        }
        widgets
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn shortcuts_help_contains_all_sections_and_valid_accelerators() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let locale = eventix_locale::default();
        let window = build(locale.as_ref());
        window.present();
        gtk::glib::MainContext::default().block_on(gtk::glib::timeout_future(
            std::time::Duration::from_millis(200),
        ));
        let widgets = descendants(window.upcast_ref());
        let sections: Vec<_> = widgets
            .iter()
            .filter(|widget| widget.has_css_class("ev_shortcuts_section"))
            .collect();
        assert_eq!(sections.len(), SECTIONS.len());
        for expected in SECTIONS {
            let section = sections
                .iter()
                .find(|section| section.widget_name() == expected.name)
                .unwrap();
            assert!(section.is_visible());
            assert_eq!(
                section
                    .first_child()
                    .unwrap()
                    .downcast::<gtk::Label>()
                    .unwrap()
                    .text(),
                expected.title
            );
            let children = descendants(section);
            let rows: Vec<_> = children
                .iter()
                .filter(|widget| widget.has_css_class("ev_shortcuts_row"))
                .collect();
            assert_eq!(rows.len(), expected.shortcuts.len() + expected.mouse.len());
            for (accelerators, title) in expected.shortcuts {
                assert!(rows.iter().any(|row| {
                    let children = descendants(row);
                    children.iter().any(|child| {
                        child
                            .downcast_ref::<gtk::Label>()
                            .is_some_and(|label| label.text() == *title)
                    }) && children.iter().any(|child| {
                        child
                            .downcast_ref::<gtk::ShortcutLabel>()
                            .is_some_and(|label| {
                                label.accelerator().as_deref() == Some(accelerators)
                            })
                    })
                }));
                for accelerator in accelerators.split_whitespace() {
                    assert!(
                        gtk::accelerator_parse(accelerator).is_some(),
                        "{accelerator}"
                    );
                }
            }
            for (title, subtitle) in expected.mouse {
                assert!(rows.iter().any(|row| {
                    let children = descendants(row);
                    [title, subtitle].iter().all(|text| {
                        children.iter().any(|child| {
                            child
                                .downcast_ref::<gtk::Label>()
                                .is_some_and(|label| label.text() == **text)
                        })
                    }) && children.iter().any(|child| child.is::<gtk::Image>())
                }));
            }
        }
        let scroll = window
            .child()
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        assert_eq!(scroll.hscrollbar_policy(), gtk::PolicyType::Never);
        assert!(scroll.vadjustment().upper() > scroll.vadjustment().page_size());
        for pair in sections.windows(2) {
            assert!(pair[0].allocation().y() < pair[1].allocation().y());
        }
        window.destroy();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn shortcuts_help_filters_keyboard_mouse_and_sections_and_resets_on_reopen() {
        gtk::init().unwrap();
        let locale = eventix_locale::default();
        let window = build(locale.as_ref());
        window.present();
        let widgets = descendants(window.upcast_ref());
        let search = descendants(&window.titlebar().unwrap())
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::SearchEntry>().ok())
            .unwrap();
        let sections: Vec<_> = widgets
            .iter()
            .filter(|widget| widget.has_css_class("ev_shortcuts_section"))
            .collect();
        let empty = widgets
            .iter()
            .find_map(|widget| {
                widget
                    .downcast_ref::<gtk::Label>()
                    .filter(|label| label.text() == "No shortcuts found")
            })
            .unwrap();

        search.set_text("mOnThLy drag");
        let visible: Vec<_> = sections
            .iter()
            .filter(|section| section.is_visible())
            .collect();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].widget_name(), "monthly");
        assert!(!empty.is_visible());

        search.set_text("F5");
        assert_eq!(
            sections
                .iter()
                .filter(|section| section.is_visible())
                .count(),
            1
        );
        assert!(
            sections
                .iter()
                .find(|section| section.widget_name() == "general")
                .unwrap()
                .is_visible()
        );

        search.set_text("no matching interaction");
        assert!(sections.iter().all(|section| !section.is_visible()));
        assert!(empty.is_visible());

        window.close();
        window.present();
        assert!(search.text().is_empty());
        assert!(sections.iter().all(|section| section.is_visible()));
        search.set_text("double-click");
        assert_eq!(
            sections
                .iter()
                .filter(|section| section.is_visible())
                .count(),
            2
        );
        window.close();
        window.present();
        assert!(search.text().is_empty());
        assert!(sections.iter().all(|section| section.is_visible()));
        window.destroy();
    }
}
