// SPDX-License-Identifier: AGPL-3.0-or-later

//! The small pieces every pane draws with: project dots, type icons, badges and labels.

use std::f64::consts::PI;

use pigeonnier_core::viewmodel::ItemVm;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::cairo;

use crate::markdown;

fn dot_color(color: &str) -> &str {
    match color {
        "blue" => "#3584e4",
        "green" => "#2ec27e",
        "yellow" => "#e5a50a",
        "orange" => "#ff7800",
        "red" => "#e01b24",
        "purple" => "#9141ac",
        "brown" => "#986a44",
        "gray" => "#77767b",
        other => other,
    }
}

pub(crate) fn escape(text: &str) -> String {
    gtk::glib::markup_escape_text(text).to_string()
}

fn colored_dot(color: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_markup(&format!(
        "<span foreground=\"{}\">●</span>",
        escape(dot_color(color))
    ));
    label.add_css_class(class);
    label
}

pub(crate) fn dot(color: &str) -> gtk::Label {
    colored_dot(color, "project-dot")
}

/// Four small dots in the colors of the first projects: all of them.
pub(crate) fn all_mark(colors: &[String]) -> gtk::Grid {
    const FALLBACK: [&str; 4] = ["blue", "green", "orange", "purple"];
    let grid = gtk::Grid::new();
    grid.set_valign(gtk::Align::Center);
    let chosen = colors.iter().map(String::as_str).chain(FALLBACK).take(4);
    for (index, color) in (0..).zip(chosen) {
        grid.attach(
            &colored_dot(color, "project-dot-small"),
            index % 2,
            index / 2,
            1,
            1,
        );
    }
    grid
}

/// Draws the icon of a type in a 12 by 12 box, with the color of the widget.
fn draw_type(context: &cairo::Context, type_id: &str) {
    let circle = |x: f64, y: f64, radius: f64| {
        context.new_sub_path();
        context.arc(x, y, radius, 0.0, 2.0 * PI);
    };
    match type_id {
        "pr" => {
            circle(3.0, 2.5, 1.3);
            circle(3.0, 9.5, 1.3);
            circle(9.0, 9.5, 1.3);
            context.move_to(3.0, 4.0);
            context.line_to(3.0, 8.0);
            context.move_to(9.0, 8.0);
            context.line_to(9.0, 5.0);
            context.curve_to(9.0, 3.9, 8.1, 3.0, 7.0, 3.0);
            context.line_to(5.0, 3.0);
        }
        "issue" => {
            circle(6.0, 6.0, 4.5);
            circle(6.0, 6.0, 0.6);
        }
        "task" => {
            let (left, top, size, radius) = (1.5, 1.5, 9.0, 2.0);
            context.new_sub_path();
            context.arc(left + size - radius, top + radius, radius, -PI / 2.0, 0.0);
            context.arc(
                left + size - radius,
                top + size - radius,
                radius,
                0.0,
                PI / 2.0,
            );
            context.arc(left + radius, top + size - radius, radius, PI / 2.0, PI);
            context.arc(left + radius, top + radius, radius, PI, 1.5 * PI);
            context.close_path();
            context.move_to(3.8, 6.0);
            context.line_to(5.4, 7.6);
            context.line_to(8.4, 4.5);
        }
        _ => {
            context.move_to(2.5, 10.5);
            context.line_to(2.5, 1.5);
            context.move_to(2.5, 2.0);
            context.line_to(9.0, 2.0);
            context.line_to(7.5, 4.0);
            context.line_to(9.0, 6.0);
            context.line_to(2.5, 6.0);
        }
    }
}

/// The colored square that says what an item is. It always takes the same room, so that titles
/// line up, and it is drawn rather than loaded: it follows the theme with no icon to install.
pub(crate) fn type_icon(type_id: &str, label: &str, size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_valign(gtk::Align::Center);
    area.set_halign(gtk::Align::Center);
    area.add_css_class("type-icon");
    area.add_css_class(&format!("type-{type_id}"));
    area.set_accessible_role(gtk::AccessibleRole::Img);
    area.set_tooltip_text(Some(label));
    area.update_property(&[gtk::accessible::Property::Label(label)]);
    let type_id = type_id.to_owned();
    area.set_draw_func(move |area, context, width, height| {
        let color = area.color();
        let scale = f64::from(width.min(height)) / 22.0;
        context.translate(
            (f64::from(width) - 12.0 * scale) / 2.0,
            (f64::from(height) - 12.0 * scale) / 2.0,
        );
        context.scale(scale, scale);
        context.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            f64::from(color.alpha()),
        );
        context.set_line_width(1.4);
        context.set_line_cap(cairo::LineCap::Round);
        context.set_line_join(cairo::LineJoin::Round);
        draw_type(context, &type_id);
        // A drawing that fails leaves the square empty; there is nothing to report.
        let _ = context.stroke();
    });
    area
}

pub(crate) fn flag(item: &ItemVm) -> gtk::Label {
    let label = gtk::Label::new(Some(&item.priority));
    let color = item.flag_color.as_deref().unwrap_or("none");
    label.add_css_class("flag");
    label.add_css_class(&format!("flag-{color}"));
    label.set_tooltip_text(Some(&item.flag_label));
    label.update_property(&[gtk::accessible::Property::Label(&item.flag_label)]);
    label
}

/// A small rounded label; `tone` is `red` or `green` when the text calls for it.
pub(crate) fn badge(text: &str, tone: Option<&str>) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("state-badge");
    if let Some(tone) = tone {
        label.add_css_class(&format!("tone-{tone}"));
    }
    label.set_valign(gtk::Align::Center);
    label
}

/// The date of an item, colored by how close it is; `short` where room is short.
pub(crate) fn due(item: &ItemVm, short: bool) -> Option<gtk::Label> {
    let text = if short {
        &item.due_short
    } else {
        &item.due_label
    };
    let label = gtk::Label::new(Some(text.as_deref()?));
    label.add_css_class("caption");
    label.add_css_class(match item.due_urgency.as_deref() {
        Some("overdue") => "error",
        Some("today") => "warning",
        Some("soon") => "heading",
        _ => "dim-label",
    });
    Some(label)
}

pub(crate) fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(tooltip));
    button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    button
}

/// A button that names its shortcut after its label.
pub(crate) fn shortcut_button(text: &str, shortcut: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.append(&gtk::Label::new(Some(text)));
    let key = gtk::Label::new(Some(shortcut));
    key.add_css_class("shortcut");
    content.append(&key);
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.update_property(&[gtk::accessible::Property::Label(text)]);
    button
}

pub(crate) fn text_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label
}

pub(crate) fn dim_label(text: &str) -> gtk::Label {
    let label = text_label(text);
    label.add_css_class("dim-label");
    label
}

/// One line that is cut with an ellipsis instead of pushing its neighbors away.
pub(crate) fn line_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    // Without this, a long text still asks for its whole width.
    label.set_width_chars(4);
    label.set_max_width_chars(8);
    label
}

pub(crate) fn markdown_label(markdown: &str) -> gtk::Label {
    let label = text_label("");
    label.set_markup(&markdown::to_pango(markdown));
    label.set_selectable(true);
    label
}

pub(crate) fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
