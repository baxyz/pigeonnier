// SPDX-License-Identifier: AGPL-3.0-or-later

//! Draws what the daemon prepares and sends actions back. The narrow layout is the sidebar: it is a
//! breakpoint of this one window, not a second window to maintain.

mod client;

use futures_util::StreamExt as _;
use pigeonnier_core::viewmodel::{ItemVm, ProjectVm, Snapshot};
use relm4::adw::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent, adw, gtk};

/// Not the name of the daemon, which owns `io.github.baxyz.Pigeonnier` on the bus.
const APP_ID: &str = "io.github.baxyz.Pigeonnier.Gnome";
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
    Mode(String),
    Key(char),
    Compact(bool),
}

struct App {
    window: adw::ApplicationWindow,
    split: adw::NavigationSplitView,
    banner: adw::Banner,
    header: adw::HeaderBar,
    views_bar: gtk::CenterBox,
    views: gtk::Box,
    compact: bool,
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

fn escape(text: &str) -> String {
    gtk::glib::markup_escape_text(text).to_string()
}

fn dot(color: &str) -> gtk::Label {
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
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
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
                eprintln!("pigeonnier-gnome: {error}");
                "Le démon ne répond pas : lance pigeonnierd".to_owned()
            },
        );
        self.banner.set_title(&escape(&text));
        self.banner.set_revealed(!text.is_empty());
    }

    /// In the narrow layout the filters no longer fit in the header, so they get a bar of their own.
    fn place_views(&self) {
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
        sender.input(Msg::Loaded(client::load(&view).await));
    });
}

/// Follows the daemon: its changes, its requests for the window, and its coming back after a stop.
fn follow_daemon(sender: ComponentSender<App>) {
    relm4::spawn(async move {
        loop {
            if let Ok(daemon) = client::connect().await
                && let (Ok(mut changed), Ok(mut requested)) = (
                    daemon.receive_changed().await,
                    daemon.receive_window_requested().await,
                )
            {
                sender.input(Msg::Load);
                loop {
                    let next = futures_util::future::select(changed.next(), requested.next());
                    match next.await {
                        futures_util::future::Either::Left((Some(_), _)) => sender.input(Msg::Load),
                        futures_util::future::Either::Right((Some(signal), _)) => {
                            if let Ok(arguments) = signal.args() {
                                sender.input(Msg::Mode(arguments.mode.clone()));
                            }
                        }
                        _ => break,
                    }
                }
            }
            relm4::tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
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

        let banner = adw::Banner::new("");
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
        body.append(&scroll);
        let views_bar = gtk::CenterBox::new();
        views_bar.set_margin_bottom(6);
        views_bar.set_visible(false);
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&header);
        content_view.add_top_bar(&views_bar);
        content_view.set_content(Some(&body));
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
        spawn_load(&sender, model.view.clone());
        follow_daemon(sender);
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Msg, sender: ComponentSender<Self>) {
        match message {
            Msg::Load => spawn_load(&sender, self.view.clone()),
            Msg::Loaded(Ok(snapshot)) => {
                let views_changed = self
                    .snapshot
                    .as_ref()
                    .is_none_or(|known| known.views != snapshot.views);
                self.snapshot = Some(snapshot);
                if views_changed {
                    self.draw_views();
                }
                self.draw_projects();
                self.draw_list();
                self.draw_banner(None);
            }
            Msg::Loaded(Err(error)) => self.draw_banner(Some(&error)),
            Msg::Refresh => {
                relm4::spawn(async {
                    if let Err(error) = client::refresh().await {
                        eprintln!("pigeonnier-gnome: {error}");
                    }
                });
            }
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
            Msg::Complete(id) => {
                let sender = sender.clone();
                relm4::spawn(async move {
                    let result = client::complete(&id)
                        .await
                        .map_err(|error| error.to_string());
                    sender.input(Msg::ActionDone(result));
                });
            }
            Msg::Snooze(id, until) => {
                let sender = sender.clone();
                relm4::spawn(async move {
                    let result = client::snooze(&id, until)
                        .await
                        .map_err(|error| error.to_string());
                    sender.input(Msg::ActionDone(result));
                });
            }
            Msg::ActionDone(result) => {
                if let Err(error) = result {
                    self.banner.set_title(&escape(&error));
                    self.banner.set_revealed(true);
                }
                spawn_load(&sender, self.view.clone());
            }
            Msg::Mode(mode) => self.apply_mode(&mode),
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
