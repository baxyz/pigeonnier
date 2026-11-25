// SPDX-License-Identifier: AGPL-3.0-or-later

//! The "Semaine" page of the preferences: whether projects have days, and a grid of the days of
//! every project. The editor of a project shows the same days.

use pigeonnier_core::config::{Config, Day, Project};
use relm4::adw::prelude::*;
use relm4::{adw, gtk};

use crate::settings::{DefaultsEdit, Edit};
use crate::{App, Msg, escape, service};

/// How many projects are worked on each day, muted ones aside.
fn per_day(config: &Config) -> [usize; 7] {
    Day::ALL.map(|day| {
        config
            .projects
            .iter()
            .filter(|project| !project.muted && project.on(day))
            .count()
    })
}

/// "lun., jeu.", or "tous les jours".
pub fn days_text(project: &Project) -> String {
    if project.days.is_empty() {
        return "tous les jours".to_owned();
    }
    project
        .days
        .iter()
        .map(|day| day.short())
        .collect::<Vec<_>>()
        .join(" ")
}

impl App {
    pub(crate) fn week_page(&self) -> Vec<adw::PreferencesGroup> {
        let Some(config) = &self.config else {
            let group = adw::PreferencesGroup::new();
            group.add(
                &adw::ActionRow::builder()
                    .title("Lecture des réglages…")
                    .build(),
            );
            return vec![group];
        };
        let mode = adw::PreferencesGroup::new();
        let switch = adw::SwitchRow::new();
        switch.set_title("Projets par jour");
        switch.set_subtitle(
            "Chaque projet a ses jours. La barre latérale propose alors un jour, aujourd'hui d'abord, et l'accueil montre la journée.",
        );
        switch.set_active(config.defaults.by_day);
        let sender = self.sender.clone();
        switch.connect_active_notify(move |row| {
            sender.input(Msg::EditDefaults(DefaultsEdit::ByDay(row.is_active())));
        });
        mode.add(&switch);

        let week = adw::PreferencesGroup::new();
        week.set_title("Ma semaine type");
        week.set_description(Some(
            "Coche les jours de chaque projet. Une ligne vide : le projet est là tous les jours.",
        ));
        week.set_sensitive(config.defaults.by_day);
        week.add(&self.week_grid(config));
        vec![mode, week]
    }

    /// Projects in rows, days in columns, and how many projects each day has.
    fn week_grid(&self, config: &Config) -> gtk::Box {
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("card");
        frame.add_css_class("week");
        let grid = gtk::Grid::new();
        grid.set_column_spacing(4);
        grid.set_row_spacing(2);
        grid.set_margin_top(8);
        grid.set_margin_bottom(8);
        grid.set_margin_start(14);
        grid.set_margin_end(14);
        let today = service::today();
        let column = |day: Day| i32::try_from(day as usize + 1).unwrap_or(1);
        for day in Day::ALL {
            let head = gtk::Label::new(Some(&day.short().trim_end_matches('.').to_uppercase()));
            head.add_css_class("caption-heading");
            head.set_hexpand(true);
            if day == today {
                head.add_css_class("accent");
            } else {
                head.add_css_class("dim-label");
            }
            grid.attach(&head, column(day), 0, 1, 1);
        }
        let mut line = 1;
        for (index, project) in config.projects.iter().enumerate() {
            if project.muted {
                continue;
            }
            let name = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            name.append(&crate::dot(&project.color));
            let label = gtk::Label::new(Some(&project.name));
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_width_chars(14);
            label.set_max_width_chars(18);
            name.append(&label);
            grid.attach(&name, 0, line, 1, 1);
            for day in Day::ALL {
                let check = gtk::CheckButton::new();
                check.set_halign(gtk::Align::Center);
                check.set_active(project.days.contains(&day));
                check.update_property(&[gtk::accessible::Property::Label(&format!(
                    "{} le {}",
                    project.name,
                    day.name()
                ))]);
                let sender = self.sender.clone();
                check.connect_toggled(move |check| {
                    sender.input(Msg::EditProject(index, Edit::Day(day, check.is_active())));
                });
                grid.attach(&check, column(day), line, 1, 1);
            }
            line += 1;
        }
        let total = gtk::Label::new(Some("Projets ce jour-là"));
        total.set_xalign(0.0);
        total.add_css_class("dim-label");
        total.add_css_class("caption");
        total.set_margin_top(6);
        grid.attach(&total, 0, line, 1, 1);
        for (day, count) in Day::ALL.into_iter().zip(per_day(config)) {
            let label = gtk::Label::new(Some(&count.to_string()));
            label.add_css_class("caption");
            label.add_css_class("numeric");
            label.add_css_class(if day == today { "accent" } else { "dim-label" });
            label.set_margin_top(6);
            grid.attach(&label, column(day), line, 1, 1);
        }
        frame.append(&grid);
        frame
    }

    /// The days of one project, in its editor: seven round toggles.
    pub(crate) fn days_row(&self, index: usize, project: &Project) -> adw::ActionRow {
        let row = adw::ActionRow::new();
        row.set_title("Jours");
        row.set_subtitle(&escape(&if project.days.is_empty() {
            "Aucun coché : tous les jours".to_owned()
        } else {
            days_text(project)
        }));
        let days = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        days.set_valign(gtk::Align::Center);
        for day in Day::ALL {
            let toggle = gtk::ToggleButton::with_label(day.letter());
            toggle.add_css_class("circular");
            toggle.add_css_class("day-toggle");
            toggle.set_active(project.days.contains(&day));
            toggle.set_tooltip_text(Some(day.name()));
            toggle.update_property(&[gtk::accessible::Property::Label(day.name())]);
            let sender = self.sender.clone();
            toggle.connect_toggled(move |toggle| {
                sender.input(Msg::EditProject(index, Edit::Day(day, toggle.is_active())));
            });
            days.append(&toggle);
        }
        row.add_suffix(&days);
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_day_counts_its_projects_and_the_daily_ones() {
        let mut config = Config::default();
        for (index, days) in [vec![Day::Mon], vec![], vec![Day::Mon, Day::Sun]]
            .into_iter()
            .enumerate()
        {
            let mut project = crate::settings::new_project(index);
            project.days = days;
            config.projects.push(project);
        }
        assert_eq!(per_day(&config), [3, 1, 1, 1, 1, 1, 2]);
        assert_eq!(days_text(&config.projects[2]), "lun. dim.");
        assert_eq!(days_text(&config.projects[1]), "tous les jours");
    }
}
