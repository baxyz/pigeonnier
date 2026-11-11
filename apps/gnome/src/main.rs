// SPDX-License-Identifier: AGPL-3.0-or-later

//! Draws what the engine prepares and sends actions back. The narrow layout is the sidebar: it is a
//! breakpoint of this one window, not a second window to maintain.

mod service;
mod settings;

use pigeonnier_core::config::Config;
use pigeonnier_core::engine::Choices;
use pigeonnier_core::viewmodel::{ItemVm, ProjectVm, Snapshot};
use relm4::adw::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent, adw, gtk};

use crate::settings::Edit;

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
    ShowSettings,
    HideSettings,
    ConnectGithub,
    ConnectTodoist,
    GithubCode(String, String),
    OpenPage(String),
    Disconnect(&'static str),
    LoadChoices,
    ChoicesLoaded(Result<(Config, Choices), String>),
    EditProject(usize, Edit),
    AddProject,
    Propose(Option<String>),
    Expanded(usize, bool),
    SaveProjects,
    Saved(Result<(), String>),
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
    settings_open: bool,
    /// The projects being edited in the settings, saved only on "Enregistrer".
    draft: Option<Config>,
    choices: Option<Choices>,
    choices_error: Option<String>,
    loading_choices: bool,
    /// The project open in the settings, kept open when the page is drawn again.
    expanded: Option<usize>,
    projects: gtk::ListBox,
    list: gtk::Box,
    view: String,
    project: Option<String>,
    snapshot: Option<Snapshot>,
    rows: Vec<(adw::ActionRow, ItemVm)>,
    focused: Option<usize>,
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

impl App {
    fn item_row(&mut self, item: &ItemVm) -> adw::ActionRow {
        let row = adw::ActionRow::new();
        row.set_title(&escape(&item.title));
        row.set_title_lines(2);
        row.set_activatable(true);
        row.add_prefix(&flag(item));
        row.add_prefix(&gtk::Image::from_icon_name(&item.icon));
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
            row.set_subtitle(&escape(&parts.join(" · ")));
            row.set_subtitle_lines(2);
        } else {
            row.set_subtitle(&escape(&item.context));
            if let Some(text) = &item.state_label {
                let badge = gtk::Label::new(Some(text));
                badge.add_css_class("state-badge");
                badge.set_valign(gtk::Align::Center);
                if item.action_required {
                    badge.add_css_class("accent");
                }
                row.add_suffix(&badge);
            }
            if let Some(label) = due(item) {
                row.add_suffix(&label);
            }
            if item.actions.iter().any(|action| action == "snooze") {
                let button = icon_button("alarm-symbolic", "Reporter à demain");
                let (sender, id) = (self.sender.clone(), item.id.clone());
                button.connect_clicked(move |_| sender.input(Msg::Snooze(id.clone(), "tomorrow")));
                row.add_suffix(&button);
            }
        }
        if item.actions.iter().any(|action| action == "merge") {
            let button = gtk::Button::with_label("Merger");
            button.add_css_class("flat");
            button.set_valign(gtk::Align::Center);
            let (sender, id, title) = (self.sender.clone(), item.id.clone(), item.title.clone());
            button.connect_clicked(move |_| sender.input(Msg::AskMerge(id.clone(), title.clone())));
            row.add_suffix(&button);
        }
        if item.actions.iter().any(|action| action == "complete") {
            let button = icon_button("object-select-symbolic", "Marquer comme fait");
            let (sender, id) = (self.sender.clone(), item.id.clone());
            button.connect_clicked(move |_| sender.input(Msg::Complete(id.clone())));
            row.add_suffix(&button);
        }
        let (sender, url) = (self.sender.clone(), item.url.clone());
        row.connect_activated(move |_| sender.input(Msg::Open(url.clone())));
        self.rows.push((row.clone(), item.clone()));
        row
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
        self.focused = None;
        let Some(mut snapshot) = self.snapshot.clone() else {
            return;
        };
        self.place_views();
        if snapshot.needs_setup || self.settings_open {
            self.draw_settings(&snapshot);
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
        // The settings page already says the same thing.
        let on_connections = self.settings_open
            || self
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

    fn in_settings(&self) -> bool {
        self.settings_open
            || self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.needs_setup)
    }

    /// In the narrow layout the filters no longer fit in the header, so they get a bar of their own.
    /// The settings show none of the list's controls.
    fn place_views(&self) {
        let in_settings = self.in_settings();
        self.quick.set_visible(!in_settings);
        self.header.set_show_back_button(!in_settings);
        self.split.set_collapsed(in_settings || self.compact);
        if in_settings {
            self.split.set_show_content(true);
            self.views_bar.set_center_widget(None::<&gtk::Widget>);
            self.views_bar.set_visible(false);
            let title = adw::WindowTitle::new("Pigeonnier", "Réglages");
            self.header.set_title_widget(Some(&title));
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

    fn focus_row(&mut self, index: usize) {
        if let Some((row, _)) = self.rows.get(index) {
            row.grab_focus();
            self.focused = Some(index);
        }
    }

    fn on_key(&mut self, key: char) {
        if self.in_settings() {
            return;
        }
        let current = self
            .rows
            .iter()
            .position(|(row, _)| row.has_focus())
            .or(self.focused);
        let item = current
            .and_then(|index| self.rows.get(index))
            .map(|(_, item)| item.clone());
        match key {
            'j' => self.focus_row(current.map_or(0, |index| index + 1)),
            'k' => self.focus_row(current.map_or(0, |index| index.saturating_sub(1))),
            'r' => self.sender.input(Msg::Refresh),
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
        header.pack_end(&refresh);
        let settings = icon_button("emblem-system-symbolic", "Réglages");
        let settings_sender = sender.clone();
        settings.connect_clicked(move |_| settings_sender.input(Msg::ShowSettings));
        header.pack_end(&settings);

        let banner = adw::Banner::new("");
        let banner_sender = sender.clone();
        banner.connect_button_clicked(move |_| banner_sender.input(Msg::ShowSettings));

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
            settings_open: false,
            draft: None,
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
                let in_settings = self.settings_open || snapshot.needs_setup;
                if in_settings && (accounts_changed || self.choices.is_none()) {
                    sender.input(Msg::LoadChoices);
                }
                self.snapshot = Some(snapshot);
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
            Msg::ShowSettings => {
                self.settings_open = true;
                self.split.set_show_content(true);
                sender.input(Msg::LoadChoices);
                self.draw_list();
                self.draw_banner(None);
            }
            Msg::HideSettings => {
                self.settings_open = false;
                self.draft = None;
                self.choices = None;
                self.expanded = None;
                self.draw_list();
                self.draw_banner(None);
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
            Msg::GithubCode(code, url) => {
                gtk::prelude::WidgetExt::display(&self.window)
                    .clipboard()
                    .set_text(&code);
                let body =
                    format!("Entre ce code sur la page GitHub, il est déjà copié :\n\n{code}");
                let dialog = adw::AlertDialog::new(Some("Connecter GitHub"), Some(&body));
                dialog.add_response("close", "Fermer");
                dialog.add_response("open", "Ouvrir GitHub");
                dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
                dialog.set_close_response("close");
                let open = sender.clone();
                dialog.connect_response(Some("open"), move |_, _| {
                    open.input(Msg::OpenPage(url.clone()));
                });
                dialog.present(Some(&self.window));
            }
            Msg::OpenPage(url) => sender.input(Msg::Open(url)),
            Msg::Disconnect(service) => {
                run(&sender, async move { service::disconnect(service).await });
            }
            Msg::LoadChoices => {
                if !self.loading_choices {
                    self.loading_choices = true;
                    self.choices_error = None;
                    let sender = sender.clone();
                    relm4::spawn(async move {
                        let result =
                            async { Ok((service::config().await?, service::choices().await?)) };
                        sender.input(Msg::ChoicesLoaded(result.await));
                    });
                }
            }
            Msg::ChoicesLoaded(result) => {
                self.loading_choices = false;
                match result {
                    Ok((config, choices)) => {
                        if self.draft.is_none() {
                            self.draft = Some(if config.projects.is_empty() {
                                let parent = settings::guess_parent(&choices);
                                settings::propose(&choices, parent.as_deref())
                            } else {
                                config
                            });
                        }
                        self.choices = Some(choices);
                    }
                    Err(error) => self.choices_error = Some(error),
                }
                self.draw_list();
            }
            Msg::EditProject(index, edit) => {
                if let Some(draft) = &mut self.draft {
                    if matches!(edit, Edit::Remove) {
                        self.expanded = None;
                    }
                    if settings::apply(draft, index, edit) {
                        self.draw_list();
                    }
                }
            }
            Msg::AddProject => {
                if let Some(draft) = &mut self.draft {
                    let index = draft.projects.len();
                    draft.projects.push(settings::new_project(index));
                    self.expanded = Some(index);
                    self.draw_list();
                }
            }
            Msg::Propose(parent) => {
                if let (Some(draft), Some(choices)) = (&mut self.draft, &self.choices) {
                    let proposal = settings::propose(choices, parent.as_deref());
                    draft.defaults.todoist_parent = proposal.defaults.todoist_parent;
                    draft.projects = proposal.projects;
                    self.expanded = None;
                    self.draw_list();
                }
            }
            Msg::Expanded(index, open) => {
                if open {
                    self.expanded = Some(index);
                } else if self.expanded == Some(index) {
                    self.expanded = None;
                }
            }
            Msg::SaveProjects => {
                if let Some(draft) = self.draft.clone() {
                    let config = settings::finalize(draft);
                    let sender = sender.clone();
                    relm4::spawn(async move {
                        sender.input(Msg::Saved(service::save_config(config).await));
                    });
                }
            }
            Msg::Saved(Ok(())) => {
                sender.input(Msg::HideSettings);
                spawn_load(&sender, self.view.clone());
            }
            Msg::Saved(Err(error)) => sender.input(Msg::Failed(error)),
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
