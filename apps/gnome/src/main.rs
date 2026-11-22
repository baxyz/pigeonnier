// SPDX-License-Identifier: AGPL-3.0-or-later

//! Draws what the engine prepares and sends actions back. The narrow layout is the sidebar: it is a
//! breakpoint of this one window, not a second window to maintain.

mod markdown;
mod service;
mod settings;

use std::collections::{HashMap, HashSet};

use pigeonnier_core::config::Config;
use pigeonnier_core::discover::Suggestion;
use pigeonnier_core::engine::Choices;
use pigeonnier_core::viewmodel::{CommentVm, DetailsVm, ItemVm, ProjectVm, Snapshot};
use relm4::adw::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent, adw, gtk};

use crate::settings::{DefaultsEdit, Edit};

const APP_ID: &str = "io.github.baxyz.Pigeonnier";
const SIDEBAR_SIZE: (i32, i32) = (340, 900);
const FULL_SIZE: (i32, i32) = (1100, 760);

#[derive(Debug)]
enum Msg {
    Load,
    Loaded(Result<Snapshot, String>),
    Refresh,
    SetView(String),
    SelectProject(Option<String>),
    Open(String),
    /// A task was unfolded; `true` when it has comments to read.
    Unfold(String, bool),
    Fold(String),
    Comments(String, Result<Vec<CommentVm>, String>),
    Complete(String),
    Snooze(String, &'static str),
    ActionDone(Result<(), String>),
    Key(char),
    Compact(bool),
    AskMerge(String, String),
    Merge(String),
    QuickAdd(String),
    Search(String),
    Failed(String),
    /// Opens the preferences, on a page if given.
    ShowPreferences(Option<&'static str>),
    PreferencesClosed,
    About,
    ConnectGithub,
    ConnectTodoist,
    GithubCode(String, String),
    OpenPage(String),
    Disconnect(&'static str),
    LoadPreferences,
    PreferencesLoaded(Result<(Config, Choices), String>),
    EditProject(usize, Edit),
    EditDefaults(DefaultsEdit),
    AddProject,
    Propose(Option<String>),
    ApplySuggestion(usize),
    IgnoreSuggestion(String),
    /// Puts back the configuration an action replaced.
    RestoreConfig(Config),
    /// The configuration is written and in use; here are the suggestions left.
    ConfigApplied(Result<Vec<Suggestion>, String>),
    Expanded(usize, bool),
}

struct App {
    window: adw::ApplicationWindow,
    split: adw::NavigationSplitView,
    banner: adw::Banner,
    header: adw::HeaderBar,
    views_bar: gtk::CenterBox,
    views: gtk::Box,
    compact: bool,
    search_bar: gtk::SearchBar,
    search_entry: gtk::SearchEntry,
    search: String,
    quick: gtk::Entry,
    preferences: Option<settings::Preferences>,
    /// The configuration being edited in the preferences; every change is written at once.
    config: Option<Config>,
    /// The configuration changed since the preferences opened: refresh when they close.
    config_changed: bool,
    suggestions: Vec<Suggestion>,
    choices: Option<Choices>,
    choices_error: Option<String>,
    loading_choices: bool,
    /// The project open in the preferences, kept open when they are drawn again.
    expanded: Option<usize>,
    projects: gtk::ListBox,
    list: gtk::Box,
    view: String,
    project: Option<String>,
    snapshot: Option<Snapshot>,
    rows: Vec<(adw::PreferencesRow, ItemVm)>,
    focused: Option<usize>,
    /// The tasks unfolded, kept unfolded when the list is drawn again.
    unfolded: HashSet<String>,
    /// The comments read for each unfolded task, and where to show them.
    comments: HashMap<String, Result<Vec<CommentVm>, String>>,
    comment_boxes: HashMap<String, gtk::Box>,
    sender: ComponentSender<App>,
}

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

pub(crate) fn dot(color: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_markup(&format!(
        "<span foreground=\"{}\">●</span>",
        escape(dot_color(color))
    ));
    label.add_css_class("project-dot");
    label
}

fn flag(item: &ItemVm) -> gtk::Label {
    let label = gtk::Label::new(Some(&item.priority));
    let color = item.flag_color.as_deref().unwrap_or("none");
    label.add_css_class("flag");
    label.add_css_class(&format!("flag-{color}"));
    label.set_tooltip_text(Some(&item.flag_label));
    label.update_property(&[gtk::accessible::Property::Label(&item.flag_label)]);
    label
}

fn due(item: &ItemVm) -> Option<gtk::Label> {
    let label = gtk::Label::new(Some(item.due_label.as_deref()?));
    label.add_css_class("caption");
    label.add_css_class(match item.due_urgency.as_deref() {
        Some("overdue") => "error",
        Some("today") => "warning",
        Some("soon") => "heading",
        _ => "dim-label",
    });
    Some(label)
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(tooltip));
    button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    button
}

fn text_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label
}

fn markdown_label(markdown: &str) -> gtk::Label {
    let label = text_label("");
    label.set_markup(&markdown::to_pango(markdown));
    label.set_selectable(true);
    label
}

/// Shows comments, or that they are being read (`Err` with no message), or why they could not be.
fn fill_comments(target: &gtk::Box, comments: &Result<Vec<CommentVm>, String>) {
    while let Some(child) = target.first_child() {
        target.remove(&child);
    }
    let note = |text: &str| {
        let label = text_label(text);
        label.add_css_class("dim-label");
        target.append(&label);
    };
    match comments {
        Err(error) if error.is_empty() => note("Lecture des commentaires…"),
        Err(error) => note(&format!("Les commentaires n'ont pas pu être lus : {error}")),
        Ok(comments) if comments.is_empty() => note("Aucun commentaire"),
        Ok(comments) => {
            for comment in comments {
                let block = gtk::Box::new(gtk::Orientation::Vertical, 4);
                if let Some(posted) = &comment.posted {
                    let when = text_label(posted);
                    when.add_css_class("caption");
                    when.add_css_class("dim-label");
                    block.append(&when);
                }
                if !comment.text.trim().is_empty() {
                    block.append(&markdown_label(&comment.text));
                }
                if let Some(file) = &comment.attachment {
                    let text = match &file.size {
                        Some(size) => format!("📎 {} ({size})", file.name),
                        None => format!("📎 {}", file.name),
                    };
                    let link = gtk::LinkButton::with_label(&file.url, &text);
                    link.set_halign(gtk::Align::Start);
                    block.append(&link);
                }
                block.add_css_class("comment");
                target.append(&block);
            }
        }
    }
}

impl App {
    /// A row that unfolds when the item has details, and opens its page otherwise.
    fn item_row(&mut self, item: &ItemVm) -> adw::PreferencesRow {
        let Some(details) = &item.details else {
            let row = adw::ActionRow::new();
            row.set_activatable(true);
            self.fill_row(&row, item);
            let (sender, url) = (self.sender.clone(), item.url.clone());
            row.connect_activated(move |_| sender.input(Msg::Open(url.clone())));
            self.rows.push((row.clone().upcast(), item.clone()));
            return row.upcast();
        };
        let row = adw::ExpanderRow::new();
        // An unfoldable row puts each new suffix before the others: the last one comes first.
        let open = icon_button("adw-external-link-symbolic", "Ouvrir dans Todoist");
        let (sender, url) = (self.sender.clone(), item.url.clone());
        open.connect_clicked(move |_| sender.input(Msg::Open(url.clone())));
        row.add_suffix(&open);
        self.fill_row(&row, item);
        row.add_row(&self.details_row(item, details));
        if self.unfolded.contains(&item.id) {
            row.set_expanded(true);
        }
        let (sender, id) = (self.sender.clone(), item.id.clone());
        let fetch = details.has_comments != Some(false);
        row.connect_expanded_notify(move |row| {
            sender.input(if row.is_expanded() {
                Msg::Unfold(id.clone(), fetch)
            } else {
                Msg::Fold(id.clone())
            });
        });
        self.rows.push((row.clone().upcast(), item.clone()));
        row.upcast()
    }

    /// What an unfolded task shows: its description, its facts and its comments.
    fn details_row(&mut self, item: &ItemVm, details: &DetailsVm) -> gtk::ListBoxRow {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        if details.description.trim().is_empty() {
            let none = text_label("Pas de description");
            none.add_css_class("dim-label");
            content.append(&none);
        } else {
            content.append(&markdown_label(&details.description));
        }
        if !details.facts.is_empty() {
            let grid = gtk::Grid::new();
            grid.set_column_spacing(12);
            grid.set_row_spacing(4);
            for (line, fact) in (0..).zip(&details.facts) {
                let name = text_label(&fact.label);
                name.add_css_class("dim-label");
                name.set_valign(gtk::Align::Start);
                let value = text_label(&fact.value);
                value.set_selectable(true);
                value.set_hexpand(true);
                grid.attach(&name, 0, line, 1, 1);
                grid.attach(&value, 1, line, 1, 1);
            }
            content.append(&grid);
        }
        let heading = text_label("Commentaires");
        heading.add_css_class("heading");
        content.append(&heading);
        let comments = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.append(&comments);
        if details.has_comments == Some(false) {
            fill_comments(&comments, &Ok(Vec::new()));
        } else if let Some(known) = self.comments.get(&item.id) {
            fill_comments(&comments, known);
        } else {
            fill_comments(&comments, &Err(String::new()));
        }
        self.comment_boxes.insert(item.id.clone(), comments);
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.set_selectable(false);
        row.set_child(Some(&content));
        row
    }

    /// The title, subtitle, badges and buttons both kinds of rows share.
    fn fill_row(&self, row: &impl IsA<adw::PreferencesRow>, item: &ItemVm) {
        let row = row.as_ref();
        row.set_title(&escape(&item.title));
        // A plain row puts each new prefix before the others, an unfoldable one after them.
        let add_prefixes = |widgets: &[gtk::Widget]| {
            if let Some(action) = row.downcast_ref::<adw::ActionRow>() {
                for widget in widgets.iter().rev() {
                    action.add_prefix(widget);
                }
            } else if let Some(expander) = row.downcast_ref::<adw::ExpanderRow>() {
                for widget in widgets {
                    expander.add_prefix(widget);
                }
            }
        };
        let suffixes = std::cell::RefCell::new(Vec::<gtk::Widget>::new());
        let add_suffix = |widget: &gtk::Widget| suffixes.borrow_mut().push(widget.clone());
        let set_subtitle = |text: &str, lines: i32| {
            if let Some(action) = row.downcast_ref::<adw::ActionRow>() {
                action.set_subtitle(text);
                action.set_title_lines(2);
                action.set_subtitle_lines(lines);
            } else if let Some(expander) = row.downcast_ref::<adw::ExpanderRow>() {
                expander.set_subtitle(text);
                expander.set_title_lines(2);
                expander.set_subtitle_lines(lines);
            }
        };
        add_prefixes(&[
            flag(item).upcast(),
            gtk::Image::from_icon_name(&item.icon).upcast(),
        ]);
        if item.waiting {
            row.add_css_class("waiting");
        }
        if self.compact {
            let parts = [
                Some(&item.context),
                item.state_label.as_ref(),
                item.due_label.as_ref(),
            ];
            let parts: Vec<&str> = parts
                .into_iter()
                .flatten()
                .map(String::as_str)
                .filter(|text| !text.is_empty())
                .collect();
            set_subtitle(&escape(&parts.join(" · ")), 2);
        } else {
            set_subtitle(&escape(&item.context), 0);
            if let Some(text) = &item.state_label {
                let badge = gtk::Label::new(Some(text));
                badge.add_css_class("state-badge");
                badge.set_valign(gtk::Align::Center);
                if item.action_required {
                    badge.add_css_class("accent");
                }
                add_suffix(badge.upcast_ref());
            }
            if let Some(label) = due(item) {
                add_suffix(label.upcast_ref());
            }
            if item.actions.iter().any(|action| action == "snooze") {
                let button = icon_button("alarm-symbolic", "Reporter à demain");
                let (sender, id) = (self.sender.clone(), item.id.clone());
                button.connect_clicked(move |_| sender.input(Msg::Snooze(id.clone(), "tomorrow")));
                add_suffix(button.upcast_ref());
            }
        }
        if item.actions.iter().any(|action| action == "merge") {
            let button = gtk::Button::with_label("Merger");
            button.add_css_class("flat");
            button.set_valign(gtk::Align::Center);
            let (sender, id, title) = (self.sender.clone(), item.id.clone(), item.title.clone());
            button.connect_clicked(move |_| sender.input(Msg::AskMerge(id.clone(), title.clone())));
            add_suffix(button.upcast_ref());
        }
        if item.actions.iter().any(|action| action == "complete") {
            let button = icon_button("object-select-symbolic", "Marquer comme fait");
            let (sender, id) = (self.sender.clone(), item.id.clone());
            button.connect_clicked(move |_| sender.input(Msg::Complete(id.clone())));
            add_suffix(button.upcast_ref());
        }
        let suffixes = suffixes.into_inner();
        if let Some(action) = row.downcast_ref::<adw::ActionRow>() {
            for widget in &suffixes {
                action.add_suffix(widget);
            }
        } else if let Some(expander) = row.downcast_ref::<adw::ExpanderRow>() {
            suffixes
                .iter()
                .rev()
                .for_each(|widget| expander.add_suffix(widget));
        }
    }

    fn project_group(&mut self, project: &ProjectVm) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title(&escape(&project.name));
        group.set_header_suffix(Some(&dot(&project.color)));
        if project.action_count > 0 {
            group.set_description(Some(&format!("{} à traiter", project.action_count)));
        }
        if project.collapsed {
            let expander = adw::ExpanderRow::new();
            let count = project.items.len();
            expander.set_title(&if count == 1 {
                "1 élément".to_owned()
            } else {
                format!("{count} éléments")
            });
            for item in &project.items {
                expander.add_row(&self.item_row(item));
            }
            group.add(&expander);
        } else {
            for item in &project.items {
                group.add(&self.item_row(item));
            }
        }
        if let Some(bots) = &project.bots {
            let expander = adw::ExpanderRow::new();
            expander.set_title(&escape(&bots.label));
            for item in &bots.items {
                expander.add_row(&self.item_row(item));
            }
            group.add(&expander);
        }
        group
    }

    fn draw_list(&mut self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.rows.clear();
        self.comment_boxes.clear();
        self.focused = None;
        let Some(mut snapshot) = self.snapshot.clone() else {
            return;
        };
        self.place_views();
        if snapshot.needs_setup {
            self.list
                .append(&settings::welcome(&self.sender, &snapshot));
            return;
        }
        let query = self.search.to_lowercase();
        if !query.is_empty() {
            let matches = |item: &ItemVm| {
                item.title.to_lowercase().contains(&query)
                    || item.context.to_lowercase().contains(&query)
            };
            for project in &mut snapshot.projects {
                project.items.retain(|item| matches(item));
                if let Some(bots) = &mut project.bots {
                    bots.items.retain(|item| matches(item));
                }
                if project
                    .bots
                    .as_ref()
                    .is_some_and(|bots| bots.items.is_empty())
                {
                    project.bots = None;
                }
            }
            snapshot
                .projects
                .retain(|project| !project.items.is_empty() || project.bots.is_some());
        }
        let shown: Vec<&ProjectVm> = snapshot
            .projects
            .iter()
            .filter(|project| self.project.as_ref().is_none_or(|id| *id == project.id))
            .collect();
        if shown.is_empty() {
            let page = adw::StatusPage::new();
            page.set_icon_name(Some("emblem-ok-symbolic"));
            page.set_title("Rien à faire");
            page.set_vexpand(true);
            self.list.append(&page);
            return;
        }
        for project in shown {
            let group = self.project_group(project);
            self.list.append(&group);
        }
    }

    fn draw_projects(&self) {
        while let Some(child) = self.projects.first_child() {
            self.projects.remove(&child);
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let total: u32 = snapshot
            .projects
            .iter()
            .map(|project| project.action_count)
            .sum();
        let add = |id: Option<&str>, name: &str, color: Option<&str>, count: u32| {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            line.set_margin_top(6);
            line.set_margin_bottom(6);
            if let Some(color) = color {
                line.append(&dot(color));
            }
            let title = gtk::Label::new(Some(name));
            title.set_hexpand(true);
            title.set_xalign(0.0);
            line.append(&title);
            if count > 0 {
                let counter = gtk::Label::new(Some(&count.to_string()));
                counter.add_css_class("dim-label");
                line.append(&counter);
            }
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&line));
            row.set_widget_name(id.unwrap_or(""));
            self.projects.append(&row);
            if self.project.as_deref() == id {
                self.projects.select_row(Some(&row));
            }
        };
        add(None, "Tous", None, total);
        for project in &snapshot.projects {
            add(
                Some(&project.id),
                &project.name,
                Some(&project.color),
                project.action_count,
            );
        }
    }

    fn draw_views(&self) {
        while let Some(child) = self.views.first_child() {
            self.views.remove(&child);
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let mut first: Option<gtk::ToggleButton> = None;
        for view in &snapshot.views {
            let button = gtk::ToggleButton::with_label(&view.title);
            button.set_active(view.id == self.view);
            if let Some(first) = &first {
                button.set_group(Some(first));
            } else {
                first = Some(button.clone());
            }
            let (sender, id) = (self.sender.clone(), view.id.clone());
            button.connect_toggled(move |button| {
                if button.is_active() {
                    sender.input(Msg::SetView(id.clone()));
                }
            });
            self.views.append(&button);
        }
    }

    fn draw_banner(&self, error: Option<&str>) {
        let issues = self
            .snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| &snapshot.issues);
        let text = error.map_or_else(
            || {
                let parts: Vec<String> = issues
                    .iter()
                    .map(|issue| format!("{} : {}", issue.source, issue.message))
                    .collect();
                parts.join(" · ")
            },
            |error| {
                eprintln!("pigeonnier: {error}");
                error.to_owned()
            },
        );
        // The welcome page already says the same thing.
        let on_connections = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.needs_setup);
        self.banner.set_title(&escape(&text));
        self.banner
            .set_revealed(!text.is_empty() && (error.is_some() || !on_connections));
        let reconnect = error.is_none() && issues.iter().any(|issue| issue.reconnect);
        self.banner
            .set_button_label(reconnect.then_some("Connecter"));
    }

    fn needs_setup(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.needs_setup)
    }

    /// In the narrow layout the filters no longer fit in the header, so they get a bar of their own.
    /// The welcome page shows none of the list's controls.
    fn place_views(&self) {
        let welcome = self.needs_setup();
        self.quick.set_visible(!welcome);
        self.header.set_show_back_button(!welcome);
        self.split.set_collapsed(welcome || self.compact);
        if welcome {
            self.split.set_show_content(true);
            self.views_bar.set_center_widget(None::<&gtk::Widget>);
            self.views_bar.set_visible(false);
            self.header.set_title_widget(None::<&gtk::Widget>);
            return;
        }
        if self.compact {
            self.header.set_title_widget(None::<&gtk::Widget>);
            self.views_bar.set_center_widget(Some(&self.views));
        } else {
            self.views_bar.set_center_widget(None::<&gtk::Widget>);
            self.header.set_title_widget(Some(&self.views));
        }
        self.views_bar.set_visible(self.compact);
    }

    /// Writes the configuration being edited and uses it at once; `redraw` when the preferences
    /// must show the change.
    fn commit(&mut self, sender: &ComponentSender<Self>, redraw: bool) {
        let (Some(config), Some(choices)) = (&self.config, &self.choices) else {
            return;
        };
        self.config_changed = true;
        apply_config(sender, config.clone(), choices.clone());
        if redraw {
            self.draw_preferences();
        }
    }

    fn focus_row(&mut self, index: usize) {
        if let Some((row, _)) = self.rows.get(index) {
            // An unfoldable row takes the focus on its header, inside it.
            if !row.grab_focus() {
                row.child_focus(gtk::DirectionType::TabForward);
            }
            self.focused = Some(index);
        }
    }

    fn on_key(&mut self, key: char) {
        if self.needs_setup() || self.preferences.is_some() {
            return;
        }
        let current = self
            .rows
            .iter()
            .position(|(row, _)| row.has_focus() || row.focus_child().is_some())
            .or(self.focused);
        let item = current
            .and_then(|index| self.rows.get(index))
            .map(|(_, item)| item.clone());
        match key {
            'j' => self.focus_row(current.map_or(0, |index| index + 1)),
            'k' => self.focus_row(current.map_or(0, |index| index.saturating_sub(1))),
            'r' => self.sender.input(Msg::Refresh),
            'o' => {
                if let Some(item) = item {
                    self.sender.input(Msg::Open(item.url));
                }
            }
            'a' => {
                self.quick.grab_focus();
            }
            '/' => {
                self.search_bar.set_search_mode(true);
                self.search_entry.grab_focus();
            }
            'x' => {
                if let Some(item) = item
                    && item.actions.iter().any(|action| action == "complete")
                {
                    self.sender.input(Msg::Complete(item.id));
                }
            }
            's' => {
                if let Some(item) = item {
                    self.sender.input(Msg::Snooze(item.id, "tomorrow"));
                }
            }
            '1'..='9' => {
                let index = key as usize - '1' as usize;
                let project = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.projects.get(index))
                    .map(|project| project.id.clone());
                if project.is_some() {
                    self.sender.input(Msg::SelectProject(project));
                }
            }
            _ => {}
        }
    }

    /// The code to enter on GitHub, selectable, with a button that copies it and one that copies it
    /// and opens the page. The dialog stays open until the person closes it.
    fn show_github_code(&self, code: &str, url: String) {
        let clipboard = gtk::prelude::WidgetExt::display(&self.window).clipboard();
        let dialog = adw::AlertDialog::new(
            Some("Connecter GitHub"),
            Some(&escape(&format!(
                "Entre ce code sur {url}. Pigeonnier attend ta réponse, puis se connecte tout seul."
            ))),
        );
        let label = gtk::Label::new(None);
        label.set_markup(&format!(
            "<span size=\"xx-large\" weight=\"bold\" font_family=\"monospace\">{}</span>",
            escape(code)
        ));
        label.set_selectable(true);
        let copy = gtk::Button::with_label("Copier");
        copy.set_halign(gtk::Align::Center);
        copy.add_css_class("pill");
        let (copied, text) = (clipboard.clone(), code.to_owned());
        copy.connect_clicked(move |button| {
            copied.set_text(&text);
            button.set_label("Copié");
        });
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.append(&label);
        content.append(&copy);
        dialog.set_extra_child(Some(&content));
        dialog.add_response("close", "Fermer");
        dialog.add_response("open", "Copier et ouvrir GitHub");
        dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("open"));
        dialog.set_close_response("close");
        let (open, text) = (self.sender.clone(), code.to_owned());
        dialog.connect_response(Some("open"), move |_, _| {
            clipboard.set_text(&text);
            open.input(Msg::OpenPage(url.clone()));
        });
        dialog.present(Some(&self.window));
        // A selectable label that takes the focus shows its text selected: the button takes it.
        dialog.set_focus(Some(&copy));
    }

    fn apply_mode(&self, mode: &str) {
        let (width, height) = if mode == "sidebar" {
            SIDEBAR_SIZE
        } else {
            FULL_SIZE
        };
        self.window.set_default_size(width, height);
        self.split.set_show_content(true);
        self.window.present();
    }
}

/// The primary menu, as GNOME apps have it: the preferences and the about dialog.
fn main_menu(window: &adw::ApplicationWindow, sender: &ComponentSender<App>) -> gtk::MenuButton {
    let action = |name: &str, message: fn() -> Msg| {
        let action = gtk::gio::SimpleAction::new(name, None);
        let sender = sender.clone();
        action.connect_activate(move |_, _| sender.input(message()));
        window.add_action(&action);
    };
    action("preferences", || Msg::ShowPreferences(None));
    action("about", || Msg::About);
    relm4::main_application().set_accels_for_action("win.preferences", &["<Control>comma"]);
    let menu = gtk::gio::Menu::new();
    menu.append(Some("Préférences"), Some("win.preferences"));
    menu.append(Some("À propos de Pigeonnier"), Some("win.about"));
    let button = gtk::MenuButton::new();
    button.set_icon_name("open-menu-symbolic");
    button.set_menu_model(Some(&menu));
    button.set_primary(true);
    button.set_tooltip_text(Some("Menu principal"));
    button
}

/// Writes `config` and uses it at once; the answer brings the suggestions left.
fn apply_config(sender: &ComponentSender<App>, config: Config, choices: Choices) {
    let sender = sender.clone();
    relm4::spawn(async move {
        let config = settings::finalize(config);
        sender.input(Msg::ConfigApplied(
            service::apply_config(config, choices).await,
        ));
    });
}

fn spawn_load(sender: &ComponentSender<App>, view: String) {
    let sender = sender.clone();
    relm4::spawn(async move {
        sender.input(Msg::Loaded(service::load(&view).await));
    });
}

/// Runs an action of the engine and reports its failure to the window.
fn run<F>(sender: &ComponentSender<App>, action: F)
where
    F: Future<Output = Result<(), String>> + Send + 'static,
{
    let sender = sender.clone();
    relm4::spawn(async move {
        sender.input(Msg::ActionDone(action.await));
    });
}

/// Starts the engine, which tells the window when the list changes or something fails.
fn start_service(sender: &ComponentSender<App>) {
    let input = sender.input_sender().clone();
    service::start(move |event| match event {
        service::Event::Changed => input.emit(Msg::Load),
        service::Event::Failed(message) => input.emit(Msg::Failed(message)),
    });
}

impl SimpleComponent for App {
    type Init = String;
    type Input = Msg;
    type Output = ();
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let window = adw::ApplicationWindow::new(&relm4::main_application());
        window.set_title(Some("Pigeonnier"));
        window
    }

    #[allow(clippy::too_many_lines)] // Building the window is long but flat.
    fn init(
        mode: String,
        window: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        relm4::set_global_css(include_str!("style.css"));

        let projects = gtk::ListBox::new();
        projects.add_css_class("navigation-sidebar");
        let select = sender.clone();
        projects.connect_row_activated(move |_, row| {
            let name = row.widget_name();
            let id = (!name.is_empty()).then(|| name.to_string());
            select.input(Msg::SelectProject(id));
        });
        let sidebar_header = adw::HeaderBar::new();
        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.add_top_bar(&sidebar_header);
        let sidebar_scroll = gtk::ScrolledWindow::new();
        sidebar_scroll.set_child(Some(&projects));
        sidebar_view.set_content(Some(&sidebar_scroll));
        let sidebar_page = adw::NavigationPage::new(&sidebar_view, "Projets");

        let views = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        views.add_css_class("linked");
        let refresh = icon_button("view-refresh-symbolic", "Rafraîchir");
        let refresh_sender = sender.clone();
        refresh.connect_clicked(move |_| refresh_sender.input(Msg::Refresh));
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&views));
        header.pack_end(&main_menu(&window, &sender));
        header.pack_end(&refresh);

        let banner = adw::Banner::new("");
        let banner_sender = sender.clone();
        banner.connect_button_clicked(move |_| {
            banner_sender.input(Msg::ShowPreferences(Some(settings::PAGE_ACCOUNTS)));
        });

        let search_entry = gtk::SearchEntry::new();
        search_entry.set_hexpand(true);
        let search_bar = gtk::SearchBar::new();
        search_bar.set_child(Some(&search_entry));
        search_bar.connect_entry(&search_entry);
        let search_sender = sender.clone();
        search_entry.connect_search_changed(move |entry| {
            search_sender.input(Msg::Search(entry.text().to_string()));
        });

        let quick = gtk::Entry::new();
        quick.set_placeholder_text(Some("Ajouter une tâche : relire la doc vendredi p2"));
        quick.set_margin_top(6);
        quick.set_margin_bottom(6);
        quick.set_margin_start(12);
        quick.set_margin_end(12);
        let quick_sender = sender.clone();
        quick.connect_activate(move |entry| {
            let text = entry.text().trim().to_owned();
            if !text.is_empty() {
                quick_sender.input(Msg::QuickAdd(text));
                entry.set_text("");
            }
        });
        let list = gtk::Box::new(gtk::Orientation::Vertical, 18);
        list.set_margin_top(12);
        list.set_margin_bottom(18);
        list.set_margin_start(12);
        list.set_margin_end(12);
        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(900);
        clamp.set_child(Some(&list));
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_child(Some(&clamp));
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&banner);
        body.append(&search_bar);
        body.append(&scroll);
        let views_bar = gtk::CenterBox::new();
        views_bar.set_margin_bottom(6);
        views_bar.set_visible(false);
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&header);
        content_view.add_top_bar(&views_bar);
        content_view.set_content(Some(&body));
        content_view.add_bottom_bar(&quick);
        let content_page = adw::NavigationPage::new(&content_view, "Pigeonnier");

        let split = adw::NavigationSplitView::new();
        split.set_sidebar(Some(&sidebar_page));
        split.set_content(Some(&content_page));
        split.set_show_content(true);
        window.set_content(Some(&split));

        let narrow =
            adw::BreakpointCondition::parse("max-width: 560sp").expect("a valid condition");
        let breakpoint = adw::Breakpoint::new(narrow);
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        let compact = sender.clone();
        breakpoint.connect_apply(move |_| compact.input(Msg::Compact(true)));
        let wide = sender.clone();
        breakpoint.connect_unapply(move |_| wide.input(Msg::Compact(false)));
        window.add_breakpoint(breakpoint);

        let keys = gtk::EventControllerKey::new();
        let key_sender = sender.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if let Some(character) = key.to_unicode() {
                key_sender.input(Msg::Key(character));
            }
            gtk::glib::Propagation::Proceed
        });
        window.add_controller(keys);

        let present = window.clone();
        relm4::main_application().connect_activate(move |_| present.present());

        let model = App {
            window: window.clone(),
            split,
            banner,
            header,
            views_bar,
            views,
            compact: false,
            search_bar,
            search_entry,
            search: String::new(),
            quick,
            preferences: None,
            config: None,
            config_changed: false,
            suggestions: Vec::new(),
            choices: None,
            choices_error: None,
            loading_choices: false,
            expanded: None,
            projects,
            list,
            view: "all".to_owned(),
            project: None,
            snapshot: None,
            rows: Vec::new(),
            focused: None,
            unfolded: HashSet::new(),
            comments: HashMap::new(),
            comment_boxes: HashMap::new(),
            sender: sender.clone(),
        };
        model.apply_mode(&mode);
        start_service(&sender);
        ComponentParts { model, widgets: () }
    }

    #[allow(clippy::too_many_lines)] // One short arm per message.
    fn update(&mut self, message: Msg, sender: ComponentSender<Self>) {
        match message {
            Msg::Load => spawn_load(&sender, self.view.clone()),
            Msg::Loaded(Ok(snapshot)) => {
                let views_changed = self
                    .snapshot
                    .as_ref()
                    .is_none_or(|known| known.views != snapshot.views);
                let accounts_changed = self.snapshot.as_ref().is_none_or(|known| {
                    (known.github_connected, known.todoist_connected)
                        != (snapshot.github_connected, snapshot.todoist_connected)
                });
                self.snapshot = Some(snapshot);
                if self.preferences.is_some() && accounts_changed {
                    self.draw_preferences();
                    sender.input(Msg::LoadPreferences);
                }
                if views_changed {
                    self.draw_views();
                }
                self.draw_projects();
                self.draw_list();
                self.draw_banner(None);
            }
            Msg::Loaded(Err(error)) => self.draw_banner(Some(&error)),
            Msg::Refresh => service::refresh(),
            Msg::SetView(view) => {
                self.view = view;
                spawn_load(&sender, self.view.clone());
            }
            Msg::SelectProject(project) => {
                self.project = project;
                self.split.set_show_content(true);
                self.draw_projects();
                self.draw_list();
            }
            Msg::Open(url) => {
                gtk::UriLauncher::new(&url).launch(
                    Some(&self.window),
                    None::<&gtk::gio::Cancellable>,
                    |result| {
                        if let Err(error) = result {
                            eprintln!("pigeonnier-gnome: {error}");
                        }
                    },
                );
            }
            Msg::Unfold(id, fetch) => {
                self.unfolded.insert(id.clone());
                if fetch {
                    if let Some(target) = self.comment_boxes.get(&id) {
                        fill_comments(target, &Err(String::new()));
                    }
                    let sender = sender.clone();
                    relm4::spawn(async move {
                        let comments = service::comments(&id).await;
                        sender.input(Msg::Comments(id, comments));
                    });
                }
            }
            Msg::Fold(id) => {
                self.unfolded.remove(&id);
                self.comments.remove(&id);
            }
            Msg::Comments(id, comments) => {
                if self.unfolded.contains(&id) {
                    if let Some(target) = self.comment_boxes.get(&id) {
                        fill_comments(target, &comments);
                    }
                    self.comments.insert(id, comments);
                }
            }
            Msg::Complete(id) => run(&sender, async move { service::complete(&id).await }),
            Msg::Snooze(id, until) => {
                run(&sender, async move { service::snooze(&id, until).await });
            }
            Msg::Merge(id) => run(&sender, async move { service::merge(&id).await }),
            Msg::AskMerge(id, title) => {
                let dialog =
                    adw::AlertDialog::new(Some("Merger cette PR ?"), Some(&escape(&title)));
                dialog.add_response("cancel", "Annuler");
                dialog.add_response("merge", "Merger");
                dialog.set_response_appearance("merge", adw::ResponseAppearance::Suggested);
                dialog.set_close_response("cancel");
                let confirm = sender.clone();
                dialog.connect_response(Some("merge"), move |_, _| {
                    confirm.input(Msg::Merge(id.clone()));
                });
                dialog.present(Some(&self.window));
            }
            Msg::QuickAdd(text) => {
                let project = self.project.clone();
                run(
                    &sender,
                    async move { service::add_task(project, &text).await },
                );
            }
            Msg::Search(query) => {
                self.search = query;
                self.draw_list();
            }
            Msg::Failed(message) => {
                self.banner.set_title(&escape(&message));
                self.banner.set_button_label(None);
                self.banner.set_revealed(true);
            }
            Msg::ShowPreferences(page) => self.open_preferences(page),
            Msg::PreferencesClosed => {
                self.preferences = None;
                self.expanded = None;
                if std::mem::take(&mut self.config_changed) {
                    service::refresh();
                }
            }
            Msg::About => {
                let about = adw::AboutDialog::new();
                about.set_application_name("Pigeonnier");
                about.set_application_icon(APP_ID);
                about.set_version(env!("CARGO_PKG_VERSION"));
                about.set_developer_name("baxyz");
                about.set_comments("Ce qui t'attend, projet par projet.");
                about.set_website("https://github.com/baxyz/pigeonnier");
                about.set_issue_url("https://github.com/baxyz/pigeonnier/issues");
                about.set_license_type(gtk::License::Agpl30);
                about.present(Some(&self.window));
            }
            Msg::ConnectGithub => {
                let sender = sender.clone();
                relm4::spawn(async move {
                    sender.input(match service::connect_github().await {
                        Ok((code, url)) => Msg::GithubCode(code, url),
                        Err(error) => Msg::Failed(error),
                    });
                });
            }
            Msg::ConnectTodoist => {
                let sender = sender.clone();
                relm4::spawn(async move {
                    sender.input(match service::connect_todoist().await {
                        Ok(url) => Msg::OpenPage(url),
                        Err(error) => Msg::Failed(error),
                    });
                });
            }
            Msg::GithubCode(code, url) => self.show_github_code(&code, url),
            Msg::OpenPage(url) => sender.input(Msg::Open(url)),
            Msg::Disconnect(service) => {
                run(&sender, async move { service::disconnect(service).await });
            }
            Msg::LoadPreferences => {
                if !self.loading_choices {
                    self.loading_choices = true;
                    self.choices_error = None;
                    let sender = sender.clone();
                    relm4::spawn(async move {
                        let result =
                            async { Ok((service::config().await?, service::choices().await?)) };
                        sender.input(Msg::PreferencesLoaded(result.await));
                    });
                }
            }
            Msg::PreferencesLoaded(result) => {
                self.loading_choices = false;
                match result {
                    Ok((mut config, choices)) => {
                        // Edits made while the accounts were read are kept.
                        if !self.config_changed || self.config.is_none() {
                            settings::assign_ids(&mut config);
                            self.config = Some(config);
                        }
                        let (sender, wanted) = (sender.clone(), choices.clone());
                        relm4::spawn(async move {
                            sender.input(Msg::ConfigApplied(service::suggestions(wanted).await));
                        });
                        self.choices = Some(choices);
                    }
                    Err(error) => self.choices_error = Some(error),
                }
                self.draw_preferences();
            }
            Msg::EditProject(index, edit) => {
                let Some(config) = &mut self.config else {
                    return;
                };
                let previous = config.clone();
                let removed = matches!(edit, Edit::Remove);
                let redraw = settings::apply(config, index, edit);
                if removed {
                    self.expanded = None;
                    self.toast_undo("Projet retiré", previous);
                }
                self.commit(&sender, redraw);
            }
            Msg::EditDefaults(edit) => {
                let Some(config) = &mut self.config else {
                    return;
                };
                // The projects say which choice of issues they follow by default.
                let redraw = matches!(edit, DefaultsEdit::Issues(_));
                settings::apply_defaults(config, edit);
                self.commit(&sender, redraw);
            }
            Msg::AddProject => {
                let Some(config) = &mut self.config else {
                    return;
                };
                let index = config.projects.len();
                config.projects.push(settings::new_project(index));
                settings::assign_ids(config);
                self.expanded = Some(index);
                self.commit(&sender, true);
            }
            Msg::Propose(parent) => {
                let (Some(config), Some(choices)) = (&mut self.config, &self.choices) else {
                    return;
                };
                let previous = config.clone();
                let proposal = settings::propose(choices, parent.as_deref());
                config.defaults.todoist_parent = proposal.defaults.todoist_parent;
                config.projects = proposal.projects;
                settings::assign_ids(config);
                self.expanded = None;
                self.toast_undo("Projets remplacés par la proposition", previous);
                self.commit(&sender, true);
            }
            Msg::ApplySuggestion(index) => {
                let (Some(config), Some(suggestion)) =
                    (&mut self.config, self.suggestions.get(index).cloned())
                else {
                    return;
                };
                let previous = config.clone();
                suggestion.change.apply(config);
                settings::assign_ids(config);
                self.suggestions.remove(index);
                self.toast_undo("Suggestion appliquée", previous);
                self.commit(&sender, true);
            }
            Msg::IgnoreSuggestion(key) => {
                let Some(config) = &mut self.config else {
                    return;
                };
                config.ignored_suggestions.push(key.clone());
                self.suggestions.retain(|suggestion| suggestion.key != key);
                self.commit(&sender, true);
            }
            Msg::RestoreConfig(previous) => {
                self.config = Some(previous);
                self.commit(&sender, true);
            }
            Msg::ConfigApplied(Ok(suggestions)) => {
                if suggestions != self.suggestions {
                    self.suggestions = suggestions;
                    self.draw_preferences();
                }
                spawn_load(&sender, self.view.clone());
            }
            Msg::ConfigApplied(Err(error)) => sender.input(Msg::Failed(error)),
            Msg::Expanded(index, open) => {
                if open {
                    self.expanded = Some(index);
                } else if self.expanded == Some(index) {
                    self.expanded = None;
                }
            }
            Msg::ActionDone(result) => {
                if let Err(error) = result {
                    self.banner.set_title(&escape(&error));
                    self.banner.set_revealed(true);
                }
                spawn_load(&sender, self.view.clone());
            }
            Msg::Key(key) => self.on_key(key),
            Msg::Compact(compact) => {
                self.compact = compact;
                self.place_views();
                self.draw_list();
            }
        }
    }
}

fn main() {
    let sidebar = std::env::args().any(|argument| argument == "--sidebar");
    let mode = if sidebar { "sidebar" } else { "full" };
    RelmApp::new(APP_ID)
        .with_args(Vec::new())
        .run::<App>(mode.to_owned());
}
