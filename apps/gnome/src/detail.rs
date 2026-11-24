// SPDX-License-Identifier: AGPL-3.0-or-later

//! The detail of the selected item, with its actions and their shortcuts. After an action the next
//! item takes its place; the focus mode leaves this pane alone on screen.

use pigeonnier_core::viewmodel::{CommentVm, ItemVm};
use relm4::adw::prelude::*;
use relm4::{adw, gtk};

use crate::widgets::{
    badge, clear, dim_label, due, markdown_label, shortcut_button, text_label, type_icon,
};
use crate::{App, Msg};

/// Shows comments, or that they are being read (`Err` with no message), or why they could not be.
pub(crate) fn fill_comments(target: &gtk::Box, comments: &Result<Vec<CommentVm>, String>) {
    clear(target);
    let note = |text: &str| target.append(&dim_label(text));
    match comments {
        Err(error) if error.is_empty() => note("Lecture des commentaires…"),
        Err(error) => note(&format!("Les commentaires n'ont pas pu être lus : {error}")),
        Ok(comments) if comments.is_empty() => note("Aucun commentaire"),
        Ok(comments) => {
            for comment in comments {
                let block = gtk::Box::new(gtk::Orientation::Vertical, 4);
                if let Some(posted) = &comment.posted {
                    let when = dim_label(posted);
                    when.add_css_class("caption");
                    block.append(&when);
                }
                if !comment.text.trim().is_empty() {
                    block.append(&markdown_label(&comment.text));
                }
                if let Some(file) = &comment.attachment {
                    let text = match &file.size {
                        Some(size) => format!("{} ({size})", file.name),
                        None => file.name.clone(),
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
    fn detail_content(&mut self, item: &ItemVm) {
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.append(&type_icon(&item.type_id, &item.type_label, 22));
        let parts = [&item.type_label, &item.context, &item.project_name];
        let mut parts: Vec<&str> = parts
            .into_iter()
            .map(String::as_str)
            .filter(|part| !part.is_empty())
            .collect();
        // A section named after its project is said once.
        parts.dedup();
        let place = dim_label(&parts.join(" · "));
        place.set_hexpand(true);
        head.append(&place);
        self.detail.append(&head);

        let title = text_label(&item.title);
        title.add_css_class(if self.focus { "title-1" } else { "title-2" });
        title.set_selectable(true);
        self.detail.append(&title);

        let badges = adw::WrapBox::new();
        badges.set_child_spacing(8);
        badges.set_line_spacing(6);
        if item.flag_color.is_some() {
            let priority = badge(&item.flag_label, None);
            priority.add_css_class(&format!(
                "flag-{}",
                item.flag_color.as_deref().unwrap_or("none")
            ));
            badges.append(&priority);
        }
        if let Some(state) = &item.state_label {
            badges.append(&badge(state, item.state_tone.as_deref()));
        }
        if let Some(label) = due(item, false) {
            label.add_css_class("state-badge");
            badges.append(&label);
        }
        if badges.first_child().is_some() {
            self.detail.append(&badges);
        }

        match item.details.description.as_deref().map(str::trim) {
            Some("") => self.detail.append(&dim_label("Pas de description")),
            Some(description) => self.detail.append(&markdown_label(description)),
            // The page of the item holds what is not read here.
            None => {}
        }

        if !item.details.facts.is_empty() {
            let grid = gtk::Grid::new();
            grid.set_column_spacing(16);
            grid.set_row_spacing(6);
            grid.add_css_class("facts");
            for (line, fact) in (0..).zip(&item.details.facts) {
                let name = dim_label(&fact.label);
                name.set_valign(gtk::Align::Start);
                let value = text_label(&fact.value);
                value.set_selectable(true);
                value.set_hexpand(true);
                grid.attach(&name, 0, line, 1, 1);
                grid.attach(&value, 1, line, 1, 1);
            }
            self.detail.append(&grid);
        }

        if item.details.commentable {
            let heading = text_label("Commentaires");
            heading.add_css_class("heading");
            self.detail.append(&heading);
            let comments = gtk::Box::new(gtk::Orientation::Vertical, 12);
            self.detail.append(&comments);
            let known = self
                .comments
                .as_ref()
                .filter(|(id, _)| *id == item.id)
                .map(|(_, comments)| comments);
            if item.details.has_comments == Some(false) {
                fill_comments(&comments, &Ok(Vec::new()));
            } else if let Some(known) = known {
                fill_comments(&comments, known);
            } else {
                // Comments are not kept: they are read each time the task is shown.
                fill_comments(&comments, &Err(String::new()));
                let (sender, id) = (self.sender.clone(), item.id.clone());
                relm4::spawn(async move {
                    let comments = crate::service::comments(&id).await;
                    sender.input(Msg::Comments(id, comments));
                });
            }
            self.comment_box = Some((item.id.clone(), comments));
        }
    }

    fn detail_actions(&self, item: &ItemVm) {
        let can = |action: &str| item.actions.iter().any(|known| known == action);
        let act = |button: &gtk::Button, message: fn(&ItemVm) -> Msg| {
            let (sender, item) = (self.sender.clone(), item.clone());
            button.connect_clicked(move |_| sender.input(message(&item)));
            self.actions.append(button);
        };
        let open = shortcut_button(&format!("Ouvrir sur {}", item.source), "Entrée");
        open.add_css_class("suggested-action");
        act(&open, |item| Msg::Open(item.id.clone()));
        if can("merge") {
            act(&shortcut_button("Merger", "M"), |item| {
                Msg::AskMerge(item.id.clone(), item.title.clone())
            });
        }
        if can("complete") {
            act(&shortcut_button("Terminer", "X"), |item| {
                Msg::Complete(item.id.clone())
            });
        }
        if can("snooze") {
            act(&shortcut_button("Reporter à demain", "S"), |item| {
                Msg::Snooze(item.id.clone(), "tomorrow")
            });
        }
        act(&gtk::Button::with_label("Copier le lien"), |item| {
            Msg::CopyLink(item.url.clone())
        });
        // In focus mode nothing else can take the keyboard: Enter opens the item.
        if self.focus {
            open.grab_focus();
        }
    }

    pub(crate) fn draw_detail(&mut self) {
        clear(&self.detail);
        while let Some(child) = self.actions.first_child() {
            self.actions.remove(&child);
        }
        self.comment_box = None;
        let item = self.selected_item().cloned();
        let position = item
            .as_ref()
            .and_then(|item| self.order.iter().position(|known| known.id == item.id));
        let total = self.order.len();
        let view = self
            .current_entry()
            .map_or_else(String::new, |entry| entry.title.clone());
        self.detail_title.set_title(&match position {
            Some(index) => format!("{} sur {total}", index + 1),
            None => String::new(),
        });
        self.detail_title
            .set_subtitle(if self.focus { &view } else { "" });
        self.step_buttons[0].set_sensitive(position.is_some_and(|index| index > 0));
        self.step_buttons[1].set_sensitive(position.is_some_and(|index| index + 1 < total));
        self.actions.set_visible(item.is_some());
        let Some(item) = item else {
            let empty = adw::StatusPage::new();
            empty.set_icon_name(Some("emblem-ok-symbolic"));
            empty.set_title(if total == 0 {
                "Tout est traité"
            } else {
                "Rien de sélectionné"
            });
            if total == 0 {
                empty.set_description(Some("Cette liste est vide."));
            }
            empty.set_vexpand(true);
            self.detail.append(&empty);
            return;
        };
        if self.comments.as_ref().is_some_and(|(id, _)| *id != item.id) {
            self.comments = None;
        }
        self.detail_content(&item);
        self.detail_actions(&item);
    }
}
