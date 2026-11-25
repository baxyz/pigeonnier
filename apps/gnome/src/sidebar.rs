// SPDX-License-Identifier: AGPL-3.0-or-later

//! The sidebar: the home page, then saved filters. A view filters by state or date, a project by
//! project; choosing one fills the filter bar above the list. The projects scroll on their own,
//! under the rest that stays in place.

use pigeonnier_core::viewmodel::Entry;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::widgets::{all_mark, dot};
use crate::{App, HOME};

impl App {
    fn sidebar_row(
        &self,
        list: &gtk::ListBox,
        id: &str,
        title: &str,
        mark: &gtk::Widget,
        count: u32,
    ) {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(6);
        line.set_margin_bottom(6);
        // A slot of the same width for every mark, so that the names line up.
        let slot = gtk::CenterBox::new();
        slot.set_size_request(16, -1);
        slot.set_hexpand(false);
        slot.set_center_widget(Some(mark));
        line.append(&slot);
        let name = gtk::Label::new(Some(title));
        name.set_hexpand(true);
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        line.append(&name);
        if count > 0 {
            let counter = gtk::Label::new(Some(&count.to_string()));
            counter.add_css_class("dim-label");
            counter.add_css_class("numeric");
            line.append(&counter);
        }
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row.set_widget_name(id);
        list.append(&row);
        if self.entry == id {
            list.select_row(Some(&row));
        }
    }

    fn sidebar_section(&self, title: &str) {
        let label = gtk::Label::new(Some(title));
        label.set_xalign(0.0);
        label.add_css_class("heading");
        label.add_css_class("dim-label");
        label.set_margin_top(12);
        label.set_margin_bottom(4);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&label));
        row.set_activatable(false);
        row.set_selectable(false);
        row.set_can_focus(false);
        self.sidebar.append(&row);
    }

    fn sidebar_entry(&self, list: &gtk::ListBox, entry: &Entry) {
        let mark: gtk::Widget = match (&entry.icon, entry.colors.as_slice()) {
            (Some(icon), _) => gtk::Image::from_icon_name(icon).upcast(),
            (None, [color]) => dot(color).upcast(),
            (None, colors) => all_mark(colors).upcast(),
        };
        self.sidebar_row(list, &entry.id, &entry.title, &mark, entry.count);
    }

    pub(crate) fn draw_sidebar(&self) {
        for list in [&self.sidebar, &self.sidebar_projects] {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let home = gtk::Image::from_icon_name("go-home-symbolic");
        self.sidebar_row(&self.sidebar, HOME, "Accueil", home.upcast_ref(), 0);
        self.sidebar_section("Vues");
        for entry in &snapshot.sidebar.views {
            self.sidebar_entry(&self.sidebar, entry);
        }
        self.sidebar_section("Projets");
        // "Tous" stays with the views; the projects themselves scroll.
        let (all, projects) = snapshot
            .sidebar
            .projects
            .split_at(1.min(snapshot.sidebar.projects.len()));
        for entry in all {
            self.sidebar_entry(&self.sidebar, entry);
        }
        for entry in projects {
            self.sidebar_entry(&self.sidebar_projects, entry);
        }
    }
}
