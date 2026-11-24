// SPDX-License-Identifier: AGPL-3.0-or-later

//! The list and the filter bar above it. The bar always shows what the list is filtered by: tokens
//! that combine, a typed query, the types with their counts, and how the list is grouped and
//! ordered. Every row has the same anatomy, and groups are always open.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pigeonnier_core::query::{Field, Filter, GroupBy, SortBy};
use pigeonnier_core::viewmodel::{Criterion, ItemVm, Page};
use relm4::adw::prelude::*;
use relm4::{ComponentSender, adw, gtk};

use crate::widgets::{badge, dot, due, escape, flag, icon_button, line_label, type_icon};
use crate::{App, Display, Heading, Msg, margins};

/// The values of a grouping drop-down: none, then every grouping.
fn group_at(index: u32) -> Option<GroupBy> {
    index
        .checked_sub(1)
        .and_then(|index| GroupBy::ALL.get(index as usize).copied())
}

fn group_index(group: Option<GroupBy>) -> u32 {
    group
        .and_then(|group| GroupBy::ALL.iter().position(|known| *known == group))
        .map_or(0, |index| u32::try_from(index + 1).unwrap_or(0))
}

fn labelled(grid: &gtk::Grid, line: i32, text: &str, control: &impl IsA<gtk::Widget>) {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    grid.attach(&label, 0, line, 1, 1);
    control.set_halign(gtk::Align::End);
    control.set_hexpand(true);
    grid.attach(control, 1, line, 1, 1);
}

/// The menu that chooses the grouping, the order and whether put-off items are shown.
fn display_menu(sender: &ComponentSender<App>) -> Display {
    let syncing = Rc::new(Cell::new(false));
    let grid = gtk::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(16);
    margins(&grid, 8, 8);

    let names: Vec<&str> = std::iter::once("Aucun")
        .chain(GroupBy::ALL.iter().map(|group| group.label()))
        .collect();
    let group = |level: usize| {
        let drop = gtk::DropDown::from_strings(&names);
        let (sender, syncing) = (sender.clone(), Rc::clone(&syncing));
        drop.connect_selected_notify(move |drop| {
            if !syncing.get() {
                sender.input(Msg::SetGroup(level, group_at(drop.selected())));
            }
        });
        drop
    };
    let groups = [group(0), group(1)];
    labelled(&grid, 0, "Regrouper par", &groups[0]);
    labelled(&grid, 1, "puis", &groups[1]);

    let sorts: Vec<&str> = SortBy::ALL.iter().map(|sort| sort.label()).collect();
    let sort = gtk::DropDown::from_strings(&sorts);
    let (sort_sender, sort_syncing) = (sender.clone(), Rc::clone(&syncing));
    sort.connect_selected_notify(move |drop| {
        if let Some(sort) = SortBy::ALL.get(drop.selected() as usize)
            && !sort_syncing.get()
        {
            sort_sender.input(Msg::SetSort(*sort));
        }
    });
    labelled(&grid, 2, "Trier par", &sort);

    let snoozed = gtk::Switch::new();
    snoozed.set_valign(gtk::Align::Center);
    let (snooze_sender, snooze_syncing) = (sender.clone(), Rc::clone(&syncing));
    snoozed.connect_active_notify(move |switch| {
        if !snooze_syncing.get() {
            snooze_sender.input(Msg::SetSnoozed(switch.is_active()));
        }
    });
    labelled(&grid, 3, "Montrer les reportés", &snoozed);

    let remove = gtk::Button::with_label("Supprimer cette vue");
    remove.add_css_class("destructive-action");
    remove.set_margin_top(8);
    grid.attach(&remove, 0, 4, 2, 1);

    let popover = gtk::Popover::new();
    popover.set_child(Some(&grid));
    let remove_sender = sender.clone();
    let closing = popover.clone();
    remove.connect_clicked(move |_| {
        closing.popdown();
        remove_sender.input(Msg::DeleteView);
    });
    let button = gtk::MenuButton::new();
    button.set_popover(Some(&popover));
    button.set_tooltip_text(Some("Regroupement, tri et éléments reportés"));
    Display {
        button,
        groups,
        sort,
        snoozed,
        remove,
        syncing,
    }
}

/// Builds the filter bar: the tokens, the typed query, then the types and the layout.
pub(crate) fn filter_bar(
    sender: &ComponentSender<App>,
) -> (
    gtk::Box,
    adw::WrapBox,
    gtk::Box,
    gtk::SearchEntry,
    Display,
    gtk::Button,
    gtk::Button,
) {
    let bar = gtk::Box::new(gtk::Orientation::Vertical, 8);
    bar.add_css_class("filter-bar");
    margins(&bar, 8, 12);

    let tokens = adw::WrapBox::new();
    tokens.set_child_spacing(6);
    tokens.set_line_spacing(6);
    bar.append(&tokens);

    let query_entry = gtk::SearchEntry::new();
    query_entry.set_placeholder_text(Some("Texte, ou état:ci-rouge, -auteur:robot…  ( / )"));
    query_entry.set_hexpand(true);
    let changed = sender.clone();
    query_entry.connect_search_changed(move |entry| {
        changed.input(Msg::QueryText(entry.text().to_string(), false));
    });
    let entered = sender.clone();
    query_entry.connect_activate(move |entry| {
        entered.input(Msg::QueryText(entry.text().to_string(), true));
    });
    bar.append(&query_entry);

    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let types = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    types.add_css_class("linked");
    line.append(&types);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    line.append(&spacer);
    let reset = icon_button("edit-undo-symbolic", "Revenir à la vue enregistrée");
    let reset_sender = sender.clone();
    reset.connect_clicked(move |_| reset_sender.input(Msg::ResetSpec));
    line.append(&reset);
    let save = icon_button("document-save-symbolic", "Enregistrer la vue");
    let save_sender = sender.clone();
    save.connect_clicked(move |_| save_sender.input(Msg::AskSaveView));
    line.append(&save);
    let display = display_menu(sender);
    line.append(&display.button);
    bar.append(&line);
    (bar, tokens, types, query_entry, display, save, reset)
}

/// Builds the list: one row an item, the headings of the groups drawn above their first row.
pub(crate) fn list_box(
    sender: &ComponentSender<App>,
    headings: &Rc<RefCell<Vec<Option<Heading>>>>,
) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.set_activate_on_single_click(false);
    list.add_css_class("items");
    let empty = adw::StatusPage::new();
    empty.set_icon_name(Some("emblem-ok-symbolic"));
    empty.set_title("Rien ici");
    empty.set_description(Some("Aucun élément ne correspond à ces filtres."));
    list.set_placeholder(Some(&empty));

    let select = sender.clone();
    list.connect_row_selected(move |_, row| {
        if let Some(row) = row {
            select.input(Msg::Select(row.widget_name().to_string()));
        }
    });
    // A click shows the detail when the list has the window to itself. The selection alone cannot
    // tell: the list also selects the row the keyboard focus lands on.
    let click = gtk::GestureClick::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (show, clicked) = (sender.clone(), list.clone());
    click.connect_pressed(move |_, _, _, y| {
        // Truncation is fine: a row is far taller than a pixel.
        #[allow(clippy::cast_possible_truncation)]
        if let Some(row) = clicked.row_at_y(y as i32) {
            show.input(Msg::Show(row.widget_name().to_string()));
        }
    });
    list.add_controller(click);
    // Enter, or a double click, opens the page of the item.
    let open = sender.clone();
    list.connect_row_activated(move |_, row| {
        open.input(Msg::Open(row.widget_name().to_string()));
    });
    let headings = Rc::clone(headings);
    list.set_header_func(move |row, _| {
        let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
        let heading = headings.borrow().get(index).cloned().flatten();
        row.set_header(heading.as_ref().map(heading_widget).as_ref());
    });
    list
}

fn heading_widget(heading: &Heading) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
    column.add_css_class("group-heading");
    if let Some(above) = &heading.above {
        let label = line_label(above);
        label.set_max_width_chars(40);
        label.add_css_class("title-4");
        label.add_css_class("group-above");
        column.append(&label);
    }
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if let Some(color) = &heading.color {
        line.append(&dot(color));
    }
    let title = line_label(&heading.title);
    title.set_max_width_chars(40);
    title.add_css_class("heading");
    line.append(&title);
    let count = gtk::Label::new(Some(&heading.count.to_string()));
    count.add_css_class("dim-label");
    count.add_css_class("caption");
    line.append(&count);
    column.append(&line);
    column
}

/// The rows all look alike: priority, type, title over where it lives, then state and date.
fn item_row(item: &ItemVm, with_project: bool) -> gtk::ListBoxRow {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    margins(&line, 8, 12);
    line.append(&flag(item));
    line.append(&type_icon(&item.type_id, &item.type_label, 22));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = line_label(&item.title);
    title.add_css_class("row-title");
    title.set_hexpand(true);
    text.append(&title);
    let place = if with_project {
        format!("{} · {}", item.context, item.project_name)
    } else {
        item.context.clone()
    };
    let subtitle = line_label(place.trim_start_matches(" · "));
    subtitle.add_css_class("caption");
    subtitle.add_css_class("dim-label");
    text.append(&subtitle);
    line.append(&text);
    if let Some(state) = &item.state_label {
        line.append(&badge(state, item.state_tone.as_deref()));
    }
    if let Some(label) = due(item, true) {
        line.append(&label);
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));
    row.set_widget_name(&item.id);
    if item.dimmed {
        row.add_css_class("dimmed");
    }
    row
}

/// The check buttons of one field. They change `state` only: the filter is applied when the menu
/// closes, so that several values can be ticked in a row.
fn criterion_editor(criterion: &Criterion, state: &Rc<RefCell<Filter>>) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
    margins(&column, 6, 6);
    if !criterion.single {
        let verbs = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        verbs.add_css_class("linked");
        verbs.set_margin_bottom(6);
        let is = gtk::ToggleButton::with_label("est");
        let is_not = gtk::ToggleButton::with_label("n'est pas");
        is_not.set_group(Some(&is));
        is.set_active(!criterion.negate);
        is_not.set_active(criterion.negate);
        let negated = Rc::clone(state);
        is_not.connect_toggled(move |button| negated.borrow_mut().negate = button.is_active());
        verbs.append(&is);
        verbs.append(&is_not);
        column.append(&verbs);
    }
    let mut first: Option<gtk::CheckButton> = None;
    for value in &criterion.values {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let label = gtk::Label::new(Some(&value.label));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        line.append(&label);
        let count = gtk::Label::new(Some(&value.count.to_string()));
        count.add_css_class("dim-label");
        count.add_css_class("numeric");
        line.append(&count);
        let check = gtk::CheckButton::new();
        check.set_child(Some(&line));
        check.set_active(value.selected);
        if criterion.single {
            match &first {
                Some(first) => check.set_group(Some(first)),
                None => first = Some(check.clone()),
            }
        }
        let (ticked, id, single) = (Rc::clone(state), value.id.clone(), criterion.single);
        check.connect_toggled(move |check| {
            let values = &mut ticked.borrow_mut().values;
            values.retain(|known| *known != id);
            if check.is_active() {
                if single {
                    values.clear();
                }
                values.push(id.clone());
            }
        });
        column.append(&check);
    }
    column
}

fn filter_of(criterion: &Criterion) -> Filter {
    Filter {
        field: criterion.field,
        values: criterion
            .values
            .iter()
            .filter(|value| value.selected)
            .map(|value| value.id.clone())
            .collect(),
        negate: criterion.negate,
    }
}

/// A menu that edits the filters of `criteria` and applies them when it closes. With several
/// fields, they are listed on the left and the values of the chosen one on the right.
fn filter_popover(criteria: &[&Criterion], sender: &ComponentSender<App>) -> gtk::Popover {
    let states: Vec<Rc<RefCell<Filter>>> = criteria
        .iter()
        .map(|criterion| Rc::new(RefCell::new(filter_of(criterion))))
        .collect();
    let stack = gtk::Stack::new();
    stack.set_vhomogeneous(false);
    for (criterion, state) in criteria.iter().zip(&states) {
        stack.add_named(
            &criterion_editor(criterion, state),
            Some(criterion.field.key()),
        );
    }
    let values = gtk::ScrolledWindow::new();
    values.set_hscrollbar_policy(gtk::PolicyType::Never);
    values.set_propagate_natural_height(true);
    values.set_max_content_height(380);
    values.set_min_content_width(230);
    values.set_child(Some(&stack));
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    if criteria.len() > 1 {
        let fields = gtk::ListBox::new();
        fields.add_css_class("navigation-sidebar");
        for criterion in criteria {
            let label = gtk::Label::new(Some(&criterion.label));
            label.set_xalign(0.0);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&label));
            row.set_widget_name(criterion.field.key());
            fields.append(&row);
        }
        let shown = stack.clone();
        fields.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                shown.set_visible_child_name(&row.widget_name());
            }
        });
        fields.select_row(fields.row_at_index(0).as_ref());
        content.append(&fields);
        content.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    }
    content.append(&values);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&content));
    let sender = sender.clone();
    popover.connect_closed(move |_| {
        let filters = states.iter().map(|state| state.borrow().clone()).collect();
        sender.input(Msg::SetFilters(filters));
    });
    popover
}

impl App {
    /// The filter bar says what the list is filtered by, whichever sidebar entry led there.
    pub(crate) fn draw_filters(&mut self) {
        let Some(page) = self.snapshot.as_ref().map(|snapshot| snapshot.page.clone()) else {
            return;
        };
        let entry = self.current_entry().cloned();
        let changed = entry.as_ref().is_some_and(|entry| entry.spec != self.spec);
        self.list_title.set_title(
            entry
                .as_ref()
                .map_or("Pigeonnier", |entry| entry.title.as_str()),
        );
        self.list_title.set_subtitle(&match page.total {
            0 => "aucun élément".to_owned(),
            1 => "1 élément".to_owned(),
            total => format!("{total} éléments"),
        });
        self.save.set_visible(changed);
        self.reset.set_visible(changed);
        self.sync_display(&page, entry.is_some_and(|entry| entry.saved));

        // Drawing the tokens again would close the menu one of them has open.
        let drawn = format!("{:?}{:?}{:?}", page.tokens, page.criteria, page.types);
        if self.drawn_filters.as_ref() == Some(&drawn) {
            return;
        }
        self.drawn_filters = Some(drawn);
        self.draw_tokens(&page);
        self.draw_types(&page);
    }

    fn draw_tokens(&self, page: &Page) {
        while let Some(child) = self.tokens.first_child() {
            self.tokens.remove(&child);
        }
        for token in &page.tokens {
            let Some(criterion) = page
                .criteria
                .iter()
                .find(|criterion| criterion.field == token.field)
            else {
                continue;
            };
            let label = gtk::Label::new(None);
            label.set_markup(&format!(
                "<b>{}</b> <span alpha=\"65%\">{}</span> {}",
                escape(&token.label),
                escape(&token.verb),
                escape(&token.values)
            ));
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(34);
            let edit = gtk::MenuButton::new();
            edit.set_child(Some(&label));
            edit.set_popover(Some(&filter_popover(&[criterion], &self.sender)));
            edit.set_tooltip_text(Some("Changer ce filtre"));
            let remove = gtk::Button::from_icon_name("window-close-symbolic");
            let name = format!("Retirer le filtre {}", token.label);
            remove.set_tooltip_text(Some(&name));
            remove.update_property(&[gtk::accessible::Property::Label(&name)]);
            let (sender, field) = (self.sender.clone(), token.field);
            remove.connect_clicked(move |_| sender.input(Msg::RemoveFilter(field)));
            let both = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            both.add_css_class("linked");
            both.add_css_class("token");
            both.append(&edit);
            both.append(&remove);
            self.tokens.append(&both);
        }
        let add = gtk::MenuButton::new();
        add.set_label("+ Filtre");
        add.add_css_class("add-filter");
        let criteria: Vec<&Criterion> = page.criteria.iter().collect();
        add.set_popover(Some(&filter_popover(&criteria, &self.sender)));
        self.tokens.append(&add);
    }

    fn draw_types(&self, page: &Page) {
        crate::widgets::clear(&self.types);
        let mut first: Option<gtk::ToggleButton> = None;
        for tab in &page.types {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
            match &tab.type_id {
                None => content.append(&gtk::Label::new(Some(&tab.label))),
                Some(type_id) => content.append(&type_icon(type_id, &tab.label, 18)),
            }
            let count = gtk::Label::new(Some(&tab.count.to_string()));
            count.add_css_class("caption");
            count.add_css_class("numeric");
            count.add_css_class("dim-label");
            content.append(&count);
            let button = gtk::ToggleButton::new();
            button.set_child(Some(&content));
            button.set_tooltip_text(Some(&tab.label));
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "{} : {}",
                tab.label, tab.count
            ))]);
            button.set_active(tab.active);
            match &first {
                Some(first) => button.set_group(Some(first)),
                None => first = Some(button.clone()),
            }
            let (sender, id) = (self.sender.clone(), tab.id.clone());
            button.connect_clicked(move |button| {
                if button.is_active() {
                    sender.input(Msg::SetType(id.clone()));
                }
            });
            self.types.append(&button);
        }
    }

    /// Fills the layout menu from the view, without its controls answering.
    fn sync_display(&self, page: &Page, saved: bool) {
        let display = &self.display;
        display.syncing.set(true);
        for (level, drop) in display.groups.iter().enumerate() {
            drop.set_selected(group_index(page.spec.group.get(level).copied()));
        }
        display.groups[1].set_sensitive(!page.spec.group.is_empty());
        let sort = SortBy::ALL.iter().position(|sort| *sort == page.spec.sort);
        display
            .sort
            .set_selected(u32::try_from(sort.unwrap_or(0)).unwrap_or(0));
        display.snoozed.set_active(page.spec.query.snoozed);
        display.remove.set_visible(saved);
        display.syncing.set(false);
        let groups: Vec<&str> = page.spec.group.iter().map(|group| group.label()).collect();
        display.button.set_label(&if groups.is_empty() {
            "Sans groupe".to_owned()
        } else {
            groups.join(" › ")
        });
    }

    pub(crate) fn select_row(&mut self, index: usize) {
        let Some(row) = i32::try_from(index)
            .ok()
            .and_then(|index| self.list.row_at_index(index))
        else {
            return;
        };
        self.list.select_row(Some(&row));
        if self.list.focus_child().is_some() {
            row.grab_focus();
        }
        self.selected = self.order.get(index).map(|item| item.id.clone());
        self.selected_index = index;
    }

    pub(crate) fn draw_list(&mut self) {
        let Some(page) = self.snapshot.as_ref().map(|snapshot| snapshot.page.clone()) else {
            return;
        };
        // The same rows are left alone: drawing them again would lose the scroll position.
        if self.drawn_groups.as_ref() != Some(&page.groups) {
            let focused = self.list.focus_child().is_some();
            self.list.remove_all();
            // The project is named on each row unless the headings or the filter already say it.
            let one_project = page
                .spec
                .query
                .filter(Field::Project)
                .is_some_and(|filter| !filter.negate && filter.values.len() == 1);
            let with_project = !one_project && !page.spec.group.contains(&GroupBy::Project);
            let grouped = !page.spec.group.is_empty();
            let mut headings = Vec::new();
            self.order.clear();
            for group in &page.groups {
                for (index, item) in group.items.iter().enumerate() {
                    headings.push((grouped && index == 0).then(|| Heading {
                        above: group.heading.clone(),
                        title: group.title.clone(),
                        color: group.color.clone(),
                        count: group.items.len(),
                    }));
                    self.list.append(&item_row(item, with_project));
                    self.order.push(item.clone());
                }
            }
            *self.headings.borrow_mut() = headings;
            self.list.invalidate_headers();
            self.drawn_groups = Some(page.groups);
            if focused {
                self.list.grab_focus();
            }
        }
        // The selected item stays selected; when it left the list, the one that took its place is.
        let index = self
            .selected
            .as_ref()
            .and_then(|id| self.order.iter().position(|item| item.id == *id))
            .or_else(|| {
                (!self.order.is_empty()).then(|| self.selected_index.min(self.order.len() - 1))
            });
        match index {
            Some(index) => self.select_row(index),
            None => self.selected = None,
        }
        self.draw_detail();
    }
}
