// SPDX-License-Identifier: AGPL-3.0-or-later

//! The home page: four figures, a card per project with what presses, and the few items that have
//! a date. Everything on it opens a list.

use pigeonnier_core::viewmodel::{Card, DayHome, Deadline, Home, Resting, Stat};
use relm4::adw::prelude::*;
use relm4::{adw, gtk};

use crate::widgets::{badge, clear, dim_label, dot, line_label, type_icon};
use crate::{App, Layout, Msg};

/// A card that opens something: a flat button with the look of a card.
fn card_button(content: &gtk::Box) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("card");
    button.add_css_class("home-card");
    button.set_child(Some(content));
    button
}

impl App {
    fn stat(&self, stat: &Stat) -> gtk::Button {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let figure = gtk::Label::new(Some(&stat.count.to_string()));
        figure.set_xalign(0.0);
        figure.add_css_class("title-1");
        figure.add_css_class("numeric");
        if stat.alert {
            figure.add_css_class("error");
        }
        content.append(&figure);
        let label = dim_label(&stat.label);
        label.add_css_class("caption");
        content.append(&label);
        let button = card_button(&content);
        button.set_hexpand(true);
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "{} {}",
            stat.count, stat.label
        ))]);
        let (sender, spec) = (self.sender.clone(), stat.spec.clone());
        button.connect_clicked(move |_| sender.input(Msg::OpenSpec(spec.clone())));
        button
    }

    fn project_card(&self, card: &Card) -> gtk::Button {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.append(&dot(&card.color));
        let name = line_label(&card.name);
        name.add_css_class("title-4");
        name.set_hexpand(true);
        head.append(&name);
        if card.action_count > 0 {
            let count = gtk::Label::new(Some(&format!("{} à traiter", card.action_count)));
            count.add_css_class("dim-label");
            count.add_css_class("caption");
            head.append(&count);
        }
        content.append(&head);
        for line in &card.lines {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&badge(&line.tag, line.tone.as_deref()));
            let title = line_label(&line.title);
            title.set_hexpand(true);
            title.set_tooltip_text(Some(&line.title));
            row.append(&title);
            content.append(&row);
        }
        if let Some(footer) = &card.footer {
            let footer = line_label(footer);
            footer.add_css_class("dim-label");
            footer.add_css_class("caption");
            content.append(&footer);
        }
        let button = card_button(&content);
        button.set_valign(gtk::Align::Start);
        let (sender, entry) = (self.sender.clone(), card.entry.clone());
        button.connect_clicked(move |_| sender.input(Msg::SelectEntry(entry.clone())));
        button
    }

    /// A project that waits for another day, in one line: it opens with every day shown.
    fn resting(&self, project: &Resting) -> gtk::Button {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&dot(&project.color));
        let name = line_label(&project.name);
        name.set_hexpand(true);
        row.append(&name);
        if let Some(urgent) = &project.urgent {
            row.append(&badge(urgent, Some("red")));
        } else if project.action_count > 0 {
            let count = dim_label(&project.action_count.to_string());
            count.add_css_class("numeric");
            row.append(&count);
        }
        if let Some(next) = &project.next {
            let next = gtk::Label::new(Some(next));
            next.add_css_class("dim-label");
            next.add_css_class("caption");
            row.append(&next);
        }
        let button = card_button(&row);
        button.add_css_class("resting");
        let (sender, entry) = (self.sender.clone(), project.entry.clone());
        button.connect_clicked(move |_| sender.input(Msg::OpenResting(entry.clone())));
        button
    }

    /// The day on top of the home page.
    fn day_head(day: &DayHome) -> gtk::Box {
        let head = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let title = gtk::Label::new(Some(&day.title));
        title.set_xalign(0.0);
        title.add_css_class("title-2");
        head.append(&title);
        let subtitle = dim_label(&day.subtitle);
        // A wrapping text asks for its whole line unless told how wide it may be.
        subtitle.set_max_width_chars(90);
        head.append(&subtitle);
        head
    }

    /// The projects that wait for another day, under the cards.
    fn resting_section(&self, day: &DayHome) -> Option<gtk::Box> {
        if day.resting.is_empty() {
            return None;
        }
        let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let title = gtk::Label::new(Some("Pas aujourd'hui"));
        title.set_xalign(0.0);
        title.add_css_class("heading");
        section.append(&title);
        // As many in a row as there is room for, so that a narrow window does not grow.
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_homogeneous(true);
        flow.set_max_children_per_line(3);
        flow.set_column_spacing(8);
        flow.set_row_spacing(8);
        for project in &day.resting {
            flow.append(&self.resting(project));
        }
        section.append(&flow);
        Some(section)
    }

    fn deadline(&self, deadline: &Deadline) -> gtk::Button {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let day = gtk::Box::new(gtk::Orientation::Vertical, 0);
        day.set_size_request(44, -1);
        let big = gtk::Label::new(Some(&deadline.big));
        big.add_css_class("title-3");
        big.add_css_class("numeric");
        let small = gtk::Label::new(Some(&deadline.small));
        small.add_css_class("caption");
        small.add_css_class(if deadline.overdue {
            "error"
        } else {
            "dim-label"
        });
        if deadline.overdue {
            big.add_css_class("error");
        }
        day.append(&big);
        day.append(&small);
        row.append(&day);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        let title = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        title.append(&type_icon(&deadline.type_id, &deadline.type_label, 18));
        let name = line_label(&deadline.title);
        name.set_hexpand(true);
        title.append(&name);
        text.append(&title);
        let sub = line_label(&deadline.sub);
        sub.add_css_class("dim-label");
        sub.add_css_class("caption");
        text.append(&sub);
        row.append(&text);
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.add_css_class("deadline");
        button.set_child(Some(&row));
        let sender = self.sender.clone();
        let (project, item) = (deadline.project.clone(), deadline.item_id.clone());
        button.connect_clicked(move |_| {
            sender.input(Msg::Reveal(project.clone(), item.clone()));
        });
        button
    }

    fn deadlines(&self, home: &Home) -> gtk::Box {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.add_css_class("card");
        column.add_css_class("deadlines");
        column.set_valign(gtk::Align::Start);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::new(Some("Échéances"));
        title.add_css_class("title-4");
        head.append(&title);
        let hint = gtk::Label::new(Some("ce qui a une date"));
        hint.add_css_class("dim-label");
        hint.add_css_class("caption");
        head.append(&hint);
        column.append(&head);
        if home.deadlines.is_empty() {
            column.append(&dim_label("Rien n'a de date."));
        }
        for deadline in &home.deadlines {
            column.append(&self.deadline(deadline));
        }
        if let Some(undated) = &home.undated {
            let note = dim_label(undated);
            note.add_css_class("caption");
            // A wrapping text asks for its whole line unless told how narrow it may be.
            note.set_max_width_chars(30);
            note.set_margin_top(8);
            column.append(&note);
        }
        column
    }

    pub(crate) fn draw_home(&self) {
        clear(&self.home);
        let Some(home) = self.snapshot.as_ref().map(|snapshot| &snapshot.home) else {
            return;
        };
        // "Commencer" goes through what waits for me, one item at a time.
        self.home_start.set_sensitive(home.start_count > 0);
        self.home_start
            .set_tooltip_text(Some(&match home.start_count {
                0 => "Rien ne t'attend".to_owned(),
                count => format!("{count} à traiter, en mode concentré"),
            }));

        // Columns of the same width, as many as the window has room for.
        let grid = |spacing: u32| {
            let grid = gtk::Grid::new();
            grid.set_column_homogeneous(true);
            grid.set_column_spacing(spacing);
            grid.set_row_spacing(spacing);
            grid.set_hexpand(true);
            grid.set_valign(gtk::Align::Start);
            grid
        };
        if let Some(day) = &home.day {
            self.home.append(&Self::day_head(day));
        }
        let narrow = self.layout == Layout::Narrow;
        let stats = grid(12);
        let columns = if narrow { 2 } else { 4 };
        for (index, stat) in (0..).zip(&home.stats) {
            stats.attach(&self.stat(stat), index % columns, index / columns, 1, 1);
        }
        self.home.append(&stats);

        let cards = grid(14);
        let columns = if narrow { 1 } else { 2 };
        for (index, card) in (0..).zip(&home.cards) {
            cards.attach(
                &self.project_card(card),
                index % columns,
                index / columns,
                1,
                1,
            );
        }
        let deadlines = self.deadlines(home);
        let body = if self.layout == Layout::Wide {
            deadlines.set_size_request(290, -1);
            // The cards take the room; the dates keep their column.
            deadlines.set_hexpand(false);
            gtk::Box::new(gtk::Orientation::Horizontal, 18)
        } else {
            gtk::Box::new(gtk::Orientation::Vertical, 18)
        };
        let main = gtk::Box::new(gtk::Orientation::Vertical, 18);
        main.set_hexpand(true);
        if home.cards.is_empty() {
            let empty = adw::StatusPage::new();
            empty.set_icon_name(Some("emblem-ok-symbolic"));
            empty.set_title("Rien à faire");
            empty.set_hexpand(true);
            main.append(&empty);
        } else {
            main.append(&cards);
        }
        if let Some(resting) = home.day.as_ref().and_then(|day| self.resting_section(day)) {
            main.append(&resting);
        }
        body.append(&main);
        body.append(&deadlines);
        self.home.append(&body);
    }
}
