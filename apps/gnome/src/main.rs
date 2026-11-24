// SPDX-License-Identifier: AGPL-3.0-or-later

//! Draws what the engine prepares and sends actions back. One window, three panes: the sidebar of
//! saved filters, the list with its filter bar, and the detail of the selected item. The home page
//! and the focus mode are two ways of using the same window; the narrow layouts are breakpoints of
//! it, not other windows to maintain.

mod detail;
mod home;
mod list;
mod markdown;
mod service;
mod settings;
mod sidebar;
mod widgets;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pigeonnier_core::config::Config;
use pigeonnier_core::discover::Suggestion;
use pigeonnier_core::engine::Choices;
use pigeonnier_core::query::{Field, Filter, GroupBy, Query, SortBy, ViewSpec};
use pigeonnier_core::viewmodel::{CommentVm, Entry, Group, ItemVm, Snapshot};
use relm4::adw::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent, adw, gtk};

use crate::settings::{DefaultsEdit, Edit};
pub(crate) use crate::widgets::{dot, escape};

const APP_ID: &str = "io.github.baxyz.Pigeonnier";
const SIDEBAR_SIZE: (i32, i32) = (340, 900);
const FULL_SIZE: (i32, i32) = (1200, 780);
/// The sidebar entry of the home page, beside the entries of the views and the projects.
const HOME: &str = "home";
/// The entry "Commencer" opens: what waits for me.
const START: &str = "view:action";

/// How much room the window has: three panes, the list and the detail, or one pane at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Wide,
    Medium,
    Narrow,
}

#[derive(Debug)]
enum Msg {
    Load,
    Loaded(Result<Snapshot, String>),
    Refresh,
    /// A sidebar entry was chosen: the home page, a view or a project.
    SelectEntry(String),
    /// Opens a list that the home page points to, under the entry it belongs to.
    OpenSpec(ViewSpec),
    /// Opens the project of an item with that item selected.
    Reveal(String, String),
    /// Opens what waits for me on its first item, in focus mode.
    Start,
    Select(String),
    /// An item was clicked: in a narrow window, its detail takes the place of the list.
    Show(String),
    Step(i32),
    Focus(bool),
    SetFilters(Vec<Filter>),
    RemoveFilter(Field),
    SetType(Option<String>),
    SetGroup(usize, Option<GroupBy>),
    SetSort(SortBy),
    SetSnoozed(bool),
    /// The filter entry changed; `true` when Enter was pressed, which turns typed filters into
    /// tokens.
    QueryText(String, bool),
    QueryParsed(Query, bool),
    ResetSpec,
    AskSaveView,
    SaveView(String),
    ViewSaved(Result<String, String>),
    DeleteView,
    Open(String),
    CopyLink(String),
    Comments(String, Result<Vec<CommentVm>, String>),
    Complete(String),
    Snooze(String, &'static str),
    ActionDone(Result<(), String>),
    Key(char),
    Layout(Layout),
    AskMerge(String, String),
    Merge(String),
    QuickAdd(String),
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

/// The heading drawn above the first row of a group.
#[derive(Debug, Clone)]
struct Heading {
    /// The heading of the first level, when the group opens it.
    above: Option<String>,
    title: String,
    color: Option<String>,
    count: usize,
}

/// The controls that choose how the list is laid out. They are built once: a change must not close
/// the popover they live in.
struct Display {
    button: gtk::MenuButton,
    groups: [gtk::DropDown; 2],
    sort: gtk::DropDown,
    snoozed: gtk::Switch,
    remove: gtk::Button,
    /// Set while the controls are filled from the view, so that filling them changes nothing.
    syncing: Rc<Cell<bool>>,
}

struct App {
    window: adw::ApplicationWindow,
    /// The sidebar beside everything else.
    outer: adw::NavigationSplitView,
    /// The list beside the detail.
    inner: adw::NavigationSplitView,
    /// The welcome page, the home page or the list with its detail.
    stack: gtk::Stack,
    banner: adw::Banner,
    sidebar: gtk::ListBox,
    welcome: gtk::Box,
    home: gtk::Box,
    home_start: gtk::Button,
    list_title: adw::WindowTitle,
    tokens: adw::WrapBox,
    types: gtk::Box,
    query_entry: gtk::SearchEntry,
    display: Display,
    save: gtk::Button,
    reset: gtk::Button,
    list: gtk::ListBox,
    headings: Rc<RefCell<Vec<Option<Heading>>>>,
    quick: gtk::Entry,
    detail_title: adw::WindowTitle,
    detail_header: adw::HeaderBar,
    detail: gtk::Box,
    actions: adw::WrapBox,
    unfocus: gtk::Button,
    focus_button: gtk::Button,
    step_buttons: [gtk::Button; 2],
    layout: Layout,
    focus: bool,
    /// The same, for the key handler that runs before the panes.
    focused: Rc<Cell<bool>>,
    /// The sidebar entry in use, and the view shown, which the filter bar may have changed.
    entry: String,
    spec: ViewSpec,
    snapshot: Option<Snapshot>,
    /// The rows of the list, in their order, and the groups they were drawn from.
    order: Vec<ItemVm>,
    drawn_groups: Option<Vec<Group>>,
    /// What the filter bar was last drawn from: drawing it again would close an open menu.
    drawn_filters: Option<String>,
    selected: Option<String>,
    /// Where the selection was, to stay there when the selected item leaves the list.
    selected_index: usize,
    /// The comments read for the selected task, and where to show them.
    comments: Option<(String, Result<Vec<CommentVm>, String>)>,
    comment_box: Option<(String, gtk::Box)>,
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
    sender: ComponentSender<App>,
}

impl App {
    fn needs_setup(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.needs_setup)
    }

    /// The sidebar entry in use, as the engine last described it.
    fn current_entry(&self) -> Option<&Entry> {
        let sidebar = &self.snapshot.as_ref()?.sidebar;
        sidebar
            .views
            .iter()
            .chain(&sidebar.projects)
            .find(|entry| entry.id == self.entry)
    }

    fn find_entry(&self, id: &str) -> Option<Entry> {
        let sidebar = &self.snapshot.as_ref()?.sidebar;
        sidebar
            .views
            .iter()
            .chain(&sidebar.projects)
            .find(|entry| entry.id == id)
            .cloned()
    }

    fn selected_item(&self) -> Option<&ItemVm> {
        let id = self.selected.as_ref()?;
        self.order.iter().find(|item| item.id == *id)
    }

    fn load(&self) {
        spawn_load(&self.sender, self.spec.clone());
    }

    /// Uses a changed view: the filter bar shows it at once, the list follows.
    fn spec_changed(&mut self) {
        self.selected = None;
        self.selected_index = 0;
        self.load();
    }

    fn open_entry(&mut self, entry: &Entry) {
        self.entry.clone_from(&entry.id);
        self.spec = entry.spec.clone();
        self.query_entry.set_text("");
        self.selected = None;
        self.selected_index = 0;
        self.outer.set_show_content(true);
        self.inner.set_show_content(false);
        self.draw_sidebar();
        self.place();
        self.load();
    }

    /// Shows the panes the layout and the focus mode leave room for.
    fn place(&self) {
        let welcome = self.needs_setup();
        let page = if welcome {
            "welcome"
        } else if self.entry == HOME {
            "home"
        } else {
            "browse"
        };
        self.stack.set_visible_child_name(page);
        let focus = self.focus && page == "browse";
        self.focused.set(focus);
        self.outer
            .set_collapsed(welcome || focus || self.layout != Layout::Wide);
        self.inner
            .set_collapsed(focus || self.layout == Layout::Narrow);
        if welcome || focus {
            self.outer.set_show_content(true);
        }
        if focus {
            self.inner.set_show_content(true);
        }
        self.detail_header.set_show_back_button(!focus);
        self.unfocus.set_visible(focus);
        self.focus_button.set_visible(!focus);
    }

    fn set_focus(&mut self, focus: bool) {
        self.focus = focus;
        self.place();
        self.draw_detail();
        if !focus && self.layout != Layout::Narrow {
            self.inner.set_show_content(false);
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
        let on_connections = self.needs_setup();
        self.banner.set_title(&escape(&text));
        self.banner
            .set_revealed(!text.is_empty() && (error.is_some() || !on_connections));
        let reconnect = error.is_none() && issues.iter().any(|issue| issue.reconnect);
        self.banner
            .set_button_label(reconnect.then_some("Connecter"));
    }

    fn draw_welcome(&self) {
        widgets::clear(&self.welcome);
        if let Some(snapshot) = &self.snapshot
            && snapshot.needs_setup
        {
            self.welcome
                .append(&settings::welcome(&self.sender, snapshot));
        }
    }

    /// Draws everything again from what the engine prepared.
    fn draw(&mut self) {
        self.draw_sidebar();
        self.draw_welcome();
        self.draw_home();
        self.draw_filters();
        self.draw_list();
        self.draw_banner(None);
        self.place();
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

    /// Selects the row `step` away from the selected one.
    fn step(&mut self, step: i32) {
        if self.order.is_empty() {
            return;
        }
        let last = self.order.len() - 1;
        let current = self
            .selected_item()
            .and_then(|item| self.order.iter().position(|known| known.id == item.id));
        let target = match (current, step < 0) {
            (None, _) => 0,
            (Some(index), true) => index.saturating_sub(1),
            (Some(index), false) => (index + 1).min(last),
        };
        self.select_row(target);
        self.draw_detail();
    }

    /// After an action on the selected item, the next one takes its place: handling the list is
    /// handling one item after the other. `leaves` when the action takes the item off the list.
    fn advance(&mut self, id: &str, leaves: bool) {
        if self.selected.as_deref() != Some(id) {
            return;
        }
        let Some(index) = self.order.iter().position(|item| item.id == id) else {
            return;
        };
        if index + 1 < self.order.len() {
            self.select_row(index + 1);
            self.draw_detail();
            // Once the item is gone, the next one stands where it stood.
            if leaves {
                self.selected_index = index;
            }
        } else if leaves {
            self.selected_index = index.saturating_sub(1);
        }
    }

    fn on_key(&mut self, key: char) {
        if self.needs_setup() || self.preferences.is_some() {
            return;
        }
        let browsing = self.entry != HOME;
        let item = self.selected_item().filter(|_| browsing).cloned();
        let can = |action: &str| {
            item.as_ref()
                .is_some_and(|item| item.actions.iter().any(|known| known == action))
        };
        match (key, item.as_ref()) {
            ('j', _) if browsing => self.step(1),
            ('k', _) if browsing => self.step(-1),
            ('r', _) => self.sender.input(Msg::Refresh),
            ('f', _) if browsing => self.sender.input(Msg::Focus(!self.focus)),
            ('a', _) if browsing && !self.focus => {
                self.quick.grab_focus();
            }
            ('/', _) if browsing && !self.focus => {
                self.query_entry.grab_focus();
            }
            ('o', Some(item)) => self.sender.input(Msg::Open(item.id.clone())),
            ('x', Some(item)) if can("complete") => {
                self.sender.input(Msg::Complete(item.id.clone()));
            }
            ('s', Some(item)) if can("snooze") => {
                self.sender.input(Msg::Snooze(item.id.clone(), "tomorrow"));
            }
            ('m', Some(item)) if can("merge") => {
                self.sender
                    .input(Msg::AskMerge(item.id.clone(), item.title.clone()));
            }
            ('1'..='9', _) => {
                let index = key as usize - '1' as usize;
                let entry = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.sidebar.projects.get(index + 1))
                    .map(|entry| entry.id.clone());
                if let Some(entry) = entry {
                    self.sender.input(Msg::SelectEntry(entry));
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

    /// Asks for the name of the view to save; the name of a saved view replaces that view.
    fn ask_save_view(&self) {
        let dialog = adw::AlertDialog::new(
            Some("Enregistrer la vue"),
            Some("Elle s'ajoute à la barre latérale, avec ses filtres et son regroupement."),
        );
        let name = gtk::Entry::new();
        name.set_placeholder_text(Some("Revue du lundi"));
        name.set_activates_default(true);
        if let Some(entry) = self.current_entry().filter(|entry| entry.saved) {
            name.set_text(&entry.title);
        }
        dialog.set_extra_child(Some(&name));
        dialog.add_response("cancel", "Annuler");
        dialog.add_response("save", "Enregistrer");
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        let (sender, typed) = (self.sender.clone(), name.clone());
        dialog.connect_response(Some("save"), move |_, _| {
            sender.input(Msg::SaveView(typed.text().to_string()));
        });
        dialog.present(Some(&self.window));
        dialog.set_focus(Some(&name));
    }

    fn apply_mode(&self, mode: &str) {
        let (width, height) = if mode == "sidebar" {
            SIDEBAR_SIZE
        } else {
            FULL_SIZE
        };
        self.window.set_default_size(width, height);
        self.outer.set_show_content(true);
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

fn refresh_button(sender: &ComponentSender<App>) -> gtk::Button {
    let refresh = widgets::icon_button("view-refresh-symbolic", "Rafraîchir (R)");
    let sender = sender.clone();
    refresh.connect_clicked(move |_| sender.input(Msg::Refresh));
    refresh
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

fn spawn_load(sender: &ComponentSender<App>, spec: ViewSpec) {
    let sender = sender.clone();
    relm4::spawn(async move {
        sender.input(Msg::Loaded(service::load(&spec).await));
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

/// A page of a split view: a header, bars under it, a content and a bar at the bottom.
fn page(
    title: &str,
    header: &adw::HeaderBar,
    top: &[&gtk::Widget],
    content: &impl IsA<gtk::Widget>,
    bottom: Option<&gtk::Widget>,
) -> adw::NavigationPage {
    let view = adw::ToolbarView::new();
    view.add_top_bar(header);
    for bar in top {
        view.add_top_bar(*bar);
    }
    view.set_content(Some(content));
    if let Some(bar) = bottom {
        view.add_bottom_bar(bar);
    }
    adw::NavigationPage::new(&view, title)
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_child(Some(child));
    scroll
}

fn margins(widget: &impl IsA<gtk::Widget>, vertical: i32, horizontal: i32) {
    widget.set_margin_top(vertical);
    widget.set_margin_bottom(vertical);
    widget.set_margin_start(horizontal);
    widget.set_margin_end(horizontal);
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

        // The sidebar: the home page, the views and the projects.
        let sidebar = gtk::ListBox::new();
        sidebar.add_css_class("navigation-sidebar");
        let select = sender.clone();
        sidebar.connect_row_activated(move |_, row| {
            select.input(Msg::SelectEntry(row.widget_name().to_string()));
        });
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.pack_end(&main_menu(&window, &sender));
        let sidebar_page = page(
            "Pigeonnier",
            &sidebar_header,
            &[],
            &scrolled(&sidebar),
            None,
        );

        // The list, under its filter bar.
        let list_title = adw::WindowTitle::new("Pigeonnier", "");
        let list_header = adw::HeaderBar::new();
        list_header.set_title_widget(Some(&list_title));
        list_header.pack_end(&refresh_button(&sender));
        let (filters, tokens, types, query_entry, display, save, reset) = list::filter_bar(&sender);
        let headings = Rc::new(RefCell::new(Vec::new()));
        let list = list::list_box(&sender, &headings);
        let quick = gtk::Entry::new();
        quick.set_placeholder_text(Some("Ajouter une tâche : relire la doc vendredi p2"));
        margins(&quick, 6, 12);
        let quick_sender = sender.clone();
        quick.connect_activate(move |entry| {
            let text = entry.text().trim().to_owned();
            if !text.is_empty() {
                quick_sender.input(Msg::QuickAdd(text));
                entry.set_text("");
            }
        });
        let list_page = page(
            "Liste",
            &list_header,
            &[filters.upcast_ref()],
            &scrolled(&list),
            Some(quick.upcast_ref()),
        );

        // The detail of the selected item, with its actions at the bottom.
        let detail_title = adw::WindowTitle::new("", "");
        let detail_header = adw::HeaderBar::new();
        detail_header.set_title_widget(Some(&detail_title));
        let unfocus = widgets::shortcut_button("Afficher la liste", "Échap");
        unfocus.add_css_class("flat");
        unfocus.set_visible(false);
        let unfocus_sender = sender.clone();
        unfocus.connect_clicked(move |_| unfocus_sender.input(Msg::Focus(false)));
        detail_header.pack_start(&unfocus);
        let focus_button = widgets::icon_button(
            "view-fullscreen-symbolic",
            "Mode concentré : masquer la barre latérale et la liste (F)",
        );
        let focus_sender = sender.clone();
        focus_button.connect_clicked(move |_| focus_sender.input(Msg::Focus(true)));
        detail_header.pack_end(&focus_button);
        let step_button = |icon: &str, tooltip: &str, step: i32| {
            let button = widgets::icon_button(icon, tooltip);
            let sender = sender.clone();
            button.connect_clicked(move |_| sender.input(Msg::Step(step)));
            button
        };
        let step_buttons = [
            step_button("go-up-symbolic", "Élément précédent (K)", -1),
            step_button("go-down-symbolic", "Élément suivant (J)", 1),
        ];
        detail_header.pack_end(&step_buttons[1]);
        detail_header.pack_end(&step_buttons[0]);
        let detail = gtk::Box::new(gtk::Orientation::Vertical, 14);
        margins(&detail, 22, 26);
        let detail_clamp = adw::Clamp::new();
        detail_clamp.set_maximum_size(760);
        detail_clamp.set_child(Some(&detail));
        let actions = adw::WrapBox::new();
        actions.set_child_spacing(8);
        actions.set_line_spacing(8);
        actions.set_justify(adw::JustifyMode::None);
        actions.set_halign(gtk::Align::Center);
        margins(&actions, 10, 12);
        let detail_page = page(
            "Détail",
            &detail_header,
            &[],
            &scrolled(&detail_clamp),
            Some(actions.upcast_ref()),
        );

        let inner = adw::NavigationSplitView::new();
        inner.set_min_sidebar_width(380.0);
        inner.set_max_sidebar_width(520.0);
        inner.set_sidebar_width_fraction(0.45);
        inner.set_sidebar(Some(&list_page));
        inner.set_content(Some(&detail_page));

        // The home page.
        let home = gtk::Box::new(gtk::Orientation::Vertical, 18);
        margins(&home, 22, 28);
        let home_header = adw::HeaderBar::new();
        home_header.set_title_widget(Some(&adw::WindowTitle::new("Accueil", "")));
        let home_start = gtk::Button::with_label("Commencer");
        home_start.add_css_class("suggested-action");
        let start_sender = sender.clone();
        home_start.connect_clicked(move |_| start_sender.input(Msg::Start));
        home_header.pack_end(&home_start);
        home_header.pack_end(&refresh_button(&sender));
        let home_view = adw::ToolbarView::new();
        home_view.add_top_bar(&home_header);
        home_view.set_content(Some(&scrolled(&home)));

        // The welcome page, before any project exists.
        let welcome = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let welcome_clamp = adw::Clamp::new();
        welcome_clamp.set_maximum_size(900);
        welcome_clamp.set_child(Some(&welcome));
        let welcome_view = adw::ToolbarView::new();
        welcome_view.add_top_bar(&adw::HeaderBar::new());
        welcome_view.set_content(Some(&scrolled(&welcome_clamp)));

        let stack = gtk::Stack::new();
        stack.add_named(&welcome_view, Some("welcome"));
        stack.add_named(&home_view, Some("home"));
        stack.add_named(&inner, Some("browse"));
        stack.set_vexpand(true);
        // A hidden page must not ask for room: each one fits the window by itself.
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);

        let banner = adw::Banner::new("");
        let banner_sender = sender.clone();
        banner.connect_button_clicked(move |_| {
            banner_sender.input(Msg::ShowPreferences(Some(settings::PAGE_ACCOUNTS)));
        });
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&banner);
        body.append(&stack);
        let content_page = adw::NavigationPage::new(&body, "Pigeonnier");

        let outer = adw::NavigationSplitView::new();
        outer.set_min_sidebar_width(200.0);
        outer.set_max_sidebar_width(260.0);
        outer.set_sidebar(Some(&sidebar_page));
        outer.set_content(Some(&content_page));
        outer.set_show_content(true);
        window.set_content(Some(&outer));

        // The last breakpoint that fits wins, so the narrowest one comes last.
        for (condition, layout) in [
            ("max-width: 1000sp", Layout::Medium),
            ("max-width: 620sp", Layout::Narrow),
        ] {
            let condition = adw::BreakpointCondition::parse(condition).expect("a valid condition");
            let breakpoint = adw::Breakpoint::new(condition);
            let apply = sender.clone();
            breakpoint.connect_apply(move |_| apply.input(Msg::Layout(layout)));
            let unapply = sender.clone();
            breakpoint.connect_unapply(move |_| unapply.input(Msg::Layout(Layout::Wide)));
            window.add_breakpoint(breakpoint);
        }

        let keys = gtk::EventControllerKey::new();
        let key_sender = sender.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let plain = !modifiers.intersects(
                gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::ALT_MASK,
            );
            if let Some(character) = key.to_unicode().filter(|_| plain) {
                key_sender.input(Msg::Key(character.to_ascii_lowercase()));
            }
            gtk::glib::Propagation::Proceed
        });
        window.add_controller(keys);
        // In focus mode Escape brings the panes back. It is caught before the split view, which
        // would take it as "go back" and show the list alone.
        let focused = Rc::new(Cell::new(false));
        let escape_keys = gtk::EventControllerKey::new();
        escape_keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (escape_sender, in_focus) = (sender.clone(), Rc::clone(&focused));
        escape_keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape && in_focus.get() {
                escape_sender.input(Msg::Focus(false));
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        window.add_controller(escape_keys);

        let present = window.clone();
        relm4::main_application().connect_activate(move |_| present.present());

        let model = App {
            window: window.clone(),
            outer,
            inner,
            stack,
            banner,
            sidebar,
            welcome,
            home,
            home_start,
            list_title,
            tokens,
            types,
            query_entry,
            display,
            save,
            reset,
            list,
            headings,
            quick,
            detail_title,
            detail_header,
            detail,
            actions,
            unfocus,
            focus_button,
            step_buttons,
            layout: Layout::Wide,
            focus: false,
            focused,
            entry: HOME.to_owned(),
            spec: pigeonnier_core::query::everything(),
            snapshot: None,
            order: Vec::new(),
            drawn_groups: None,
            drawn_filters: None,
            selected: None,
            selected_index: 0,
            comments: None,
            comment_box: None,
            preferences: None,
            config: None,
            config_changed: false,
            suggestions: Vec::new(),
            choices: None,
            choices_error: None,
            loading_choices: false,
            expanded: None,
            sender: sender.clone(),
        };
        model.apply_mode(&mode);
        model.place();
        start_service(&sender);
        ComponentParts { model, widgets: () }
    }

    #[allow(clippy::too_many_lines)] // One short arm per message.
    fn update(&mut self, message: Msg, sender: ComponentSender<Self>) {
        match message {
            Msg::Load => self.load(),
            Msg::Loaded(Ok(snapshot)) => {
                // An answer for a view that was left since: the one asked for last is on its way.
                if snapshot.page.spec != self.spec {
                    return;
                }
                let accounts_changed = self.snapshot.as_ref().is_none_or(|known| {
                    (known.github_connected, known.todoist_connected)
                        != (snapshot.github_connected, snapshot.todoist_connected)
                });
                self.snapshot = Some(snapshot);
                if self.preferences.is_some() && accounts_changed {
                    self.draw_preferences();
                    sender.input(Msg::LoadPreferences);
                }
                self.draw();
            }
            Msg::Loaded(Err(error)) => self.draw_banner(Some(&error)),
            Msg::Refresh => service::refresh(),
            Msg::SelectEntry(id) => {
                self.focus = false;
                if id == HOME {
                    self.entry = id;
                    self.outer.set_show_content(true);
                    self.draw_sidebar();
                    self.place();
                } else if let Some(entry) = self.find_entry(&id) {
                    self.open_entry(&entry);
                }
            }
            Msg::OpenSpec(spec) => {
                // A list the sidebar already holds opens there; another one is a change to "Tous".
                let known = self.snapshot.as_ref().and_then(|snapshot| {
                    let sidebar = &snapshot.sidebar;
                    let mut entries = sidebar.views.iter().chain(&sidebar.projects);
                    entries.find(|entry| entry.spec == spec).cloned()
                });
                let entry = known.or_else(|| self.find_entry("projects"));
                if let Some(mut entry) = entry {
                    entry.spec = spec;
                    self.focus = false;
                    self.open_entry(&entry);
                }
            }
            Msg::Reveal(project, item) => {
                if let Some(entry) = self.find_entry(&format!("project:{project}")) {
                    self.focus = false;
                    self.open_entry(&entry);
                    self.selected = Some(item);
                    self.inner.set_show_content(true);
                }
            }
            Msg::Start => {
                if let Some(entry) = self.find_entry(START) {
                    self.open_entry(&entry);
                    self.focus = true;
                    self.place();
                }
            }
            Msg::Select(id) => {
                if let Some(index) = self.order.iter().position(|item| item.id == id) {
                    self.selected_index = index;
                }
                if self.selected.as_ref() != Some(&id) {
                    self.selected = Some(id);
                    self.draw_detail();
                }
            }
            Msg::Show(id) => {
                if let Some(index) = self.order.iter().position(|item| item.id == id) {
                    self.select_row(index);
                    self.draw_detail();
                    self.inner.set_show_content(true);
                    // With the window to itself, the detail gives the keyboard to its first action
                    // rather than to a text that would show itself selected.
                    if self.inner.is_collapsed()
                        && let Some(action) = self.actions.first_child()
                    {
                        action.grab_focus();
                    }
                }
            }
            Msg::Step(step) => self.step(step),
            Msg::Focus(focus) => self.set_focus(focus),
            Msg::SetFilters(filters) => {
                let before = self.spec.clone();
                for filter in filters {
                    self.spec.query.set(filter);
                }
                if self.spec != before {
                    self.spec_changed();
                }
            }
            Msg::RemoveFilter(field) => {
                self.spec.query = self.spec.query.without(field);
                self.spec_changed();
            }
            Msg::SetType(kind) => {
                let values: Vec<&str> = kind.as_deref().into_iter().collect();
                self.spec.query.set(Filter::new(Field::Type, &values));
                self.spec_changed();
            }
            Msg::SetGroup(level, by) => {
                let mut group = self.spec.group.clone();
                group.truncate(level);
                // The second level only exists under a first one, and differs from it.
                if let Some(by) = by.filter(|by| !group.contains(by)) {
                    group.push(by);
                    let below = self.spec.group.get(level + 1).copied();
                    group.extend(below.filter(|below| *below != by));
                }
                if group != self.spec.group {
                    self.spec.group = group;
                    self.spec_changed();
                }
            }
            Msg::SetSort(sort) => {
                if sort != self.spec.sort {
                    self.spec.sort = sort;
                    self.spec_changed();
                }
            }
            Msg::SetSnoozed(snoozed) => {
                if snoozed != self.spec.query.snoozed {
                    self.spec.query.snoozed = snoozed;
                    self.spec_changed();
                }
            }
            Msg::QueryText(text, entered) => {
                let sender = sender.clone();
                relm4::spawn(async move {
                    match service::parse_query(&text).await {
                        Ok(query) => sender.input(Msg::QueryParsed(query, entered)),
                        Err(error) => sender.input(Msg::Failed(error)),
                    }
                });
            }
            Msg::QueryParsed(typed, entered) => {
                let before = self.spec.clone();
                self.spec.query.text.clone_from(&typed.text);
                if entered {
                    for filter in typed.filters {
                        self.spec.query.set(filter);
                    }
                    // The filters became tokens: only the words stay in the entry.
                    if self.query_entry.text() != typed.text {
                        self.query_entry.set_text(&typed.text);
                    }
                }
                if self.spec != before {
                    self.spec_changed();
                }
            }
            Msg::ResetSpec => {
                if let Some(entry) = self.current_entry().cloned() {
                    self.open_entry(&entry);
                }
            }
            Msg::AskSaveView => self.ask_save_view(),
            Msg::SaveView(name) => {
                let (sender, spec) = (sender.clone(), self.spec.clone());
                relm4::spawn(async move {
                    sender.input(Msg::ViewSaved(service::save_view(spec, &name).await));
                });
            }
            Msg::ViewSaved(Ok(entry)) => {
                self.entry = entry;
                self.load();
            }
            Msg::ViewSaved(Err(error)) | Msg::Failed(error) => {
                self.banner.set_title(&escape(&error));
                self.banner.set_button_label(None);
                self.banner.set_revealed(true);
            }
            Msg::DeleteView => {
                let entry = self.entry.clone();
                "projects".clone_into(&mut self.entry);
                let sender = sender.clone();
                relm4::spawn(async move {
                    match service::delete_view(&entry).await {
                        Ok(()) => sender.input(Msg::Load),
                        Err(error) => sender.input(Msg::Failed(error)),
                    }
                });
            }
            Msg::Open(id) => {
                let url = self
                    .order
                    .iter()
                    .find(|item| item.id == id)
                    .map(|item| item.url.clone());
                if let Some(url) = url {
                    sender.input(Msg::OpenPage(url));
                    self.advance(&id, false);
                }
            }
            Msg::OpenPage(url) => {
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
            Msg::CopyLink(url) => {
                gtk::prelude::WidgetExt::display(&self.window)
                    .clipboard()
                    .set_text(&url);
            }
            Msg::Comments(id, comments) => {
                if self.selected.as_deref() == Some(id.as_str()) {
                    if let Some((_, target)) =
                        self.comment_box.as_ref().filter(|(known, _)| *known == id)
                    {
                        detail::fill_comments(target, &comments);
                    }
                    self.comments = Some((id, comments));
                }
            }
            Msg::Complete(id) => {
                self.advance(&id, true);
                run(&sender, async move { service::complete(&id).await });
            }
            Msg::Snooze(id, until) => {
                self.advance(&id, true);
                run(&sender, async move { service::snooze(&id, until).await });
            }
            Msg::Merge(id) => {
                self.advance(&id, true);
                run(&sender, async move { service::merge(&id).await });
            }
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
                // A list of one project adds to that project; any other one to the Todoist inbox.
                let project = self
                    .spec
                    .query
                    .filter(Field::Project)
                    .filter(|filter| !filter.negate && filter.values.len() == 1)
                    .map(|filter| filter.values[0].clone());
                run(
                    &sender,
                    async move { service::add_task(project, &text).await },
                );
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
                self.load();
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
                self.load();
            }
            Msg::Key(key) => self.on_key(key),
            Msg::Layout(layout) => {
                self.layout = layout;
                self.draw_home();
                self.place();
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
