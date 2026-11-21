// SPDX-License-Identifier: AGPL-3.0-or-later

//! Markdown, as written in Todoist, turned into the Pango markup a label shows. Labels know no
//! blocks, so headings, lists, quotes and code become text with emphasis, bullets and indents.
//! Bare web addresses become links, as Todoist shows them.

use std::fmt::Write as _;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::escape;

#[derive(Default)]
struct Writer {
    out: String,
    /// The next number of each open list, `None` for a bulleted one.
    lists: Vec<Option<u64>>,
    quotes: usize,
    links: usize,
    code_block: bool,
    /// A list item that has not yet written text: its first paragraph goes on the bullet's line.
    fresh_item: bool,
}

impl Writer {
    /// Starts a block: a blank line between top-level blocks, a line break inside a list.
    fn block(&mut self) {
        if self.fresh_item {
            self.fresh_item = false;
            return;
        }
        let trimmed = self.out.trim_end_matches('\n').len();
        if trimmed == 0 {
            self.out.clear();
            return;
        }
        self.out.truncate(trimmed);
        self.out
            .push_str(if self.lists.is_empty() { "\n\n" } else { "\n" });
        if !self.lists.is_empty() {
            self.indent(self.lists.len());
        }
        if self.quotes > 0 {
            self.out.push_str("▎ ");
        }
    }

    fn indent(&mut self, depth: usize) {
        self.out.push_str(&"    ".repeat(depth.saturating_sub(1)));
    }

    fn text(&mut self, text: &str) {
        self.fresh_item = false;
        if self.links > 0 || self.code_block {
            self.out.push_str(&escape(text));
        } else {
            self.out.push_str(&linkify(text));
        }
    }

    fn start(&mut self, tag: &Tag<'_>) {
        match tag {
            Tag::Paragraph => self.block(),
            Tag::Heading { level, .. } => {
                self.block();
                let size = if *level == HeadingLevel::H1 {
                    "x-large"
                } else {
                    "large"
                };
                let _ = write!(self.out, "<span size=\"{size}\" weight=\"bold\">");
            }
            Tag::BlockQuote(_) => {
                self.block();
                self.quotes += 1;
                self.out.push_str("▎ <i>");
                self.fresh_item = true;
            }
            Tag::CodeBlock(_) => {
                self.block();
                self.code_block = true;
                self.out.push_str("<tt>");
            }
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.block();
                    self.fresh_item = true;
                }
                self.lists.push(*start);
            }
            Tag::Item => {
                if self.fresh_item {
                    self.fresh_item = false;
                } else {
                    let trimmed = self.out.trim_end_matches('\n').len();
                    self.out.truncate(trimmed);
                    self.out.push('\n');
                }
                self.indent(self.lists.len());
                let bullet = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let bullet = format!("{number}. ");
                        *number += 1;
                        bullet
                    }
                    _ => "• ".to_owned(),
                };
                self.out.push_str(&bullet);
                self.fresh_item = true;
            }
            Tag::Emphasis => self.out.push_str("<i>"),
            Tag::Strong => self.out.push_str("<b>"),
            Tag::Strikethrough => self.out.push_str("<s>"),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.links += 1;
                let _ = write!(self.out, "<a href=\"{}\">", escape(dest_url));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => self.out.push_str("</span>"),
            TagEnd::BlockQuote(_) => {
                self.quotes = self.quotes.saturating_sub(1);
                self.fresh_item = false;
                self.out.push_str("</i>");
            }
            TagEnd::CodeBlock => {
                self.code_block = false;
                let trimmed = self.out.trim_end_matches('\n').len();
                self.out.truncate(trimmed);
                self.out.push_str("</tt>");
            }
            TagEnd::List(_) => {
                self.lists.pop();
                self.fresh_item = false;
            }
            TagEnd::Item => self.fresh_item = false,
            TagEnd::Emphasis => self.out.push_str("</i>"),
            TagEnd::Strong => self.out.push_str("</b>"),
            TagEnd::Strikethrough => self.out.push_str("</s>"),
            TagEnd::Link | TagEnd::Image => {
                self.links = self.links.saturating_sub(1);
                self.out.push_str("</a>");
            }
            _ => {}
        }
    }
}

/// Escapes `text` and makes a link of each `http://` or `https://` address in it.
fn linkify(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| rest.find(scheme))
        .min()
    {
        out.push_str(&escape(&rest[..start]));
        let address = &rest[start..];
        let mut end = address
            .find(|character: char| character.is_whitespace() || "<>\"".contains(character))
            .unwrap_or(address.len());
        // A sentence's punctuation is not part of the address.
        while end > 0 && ".,;:!?)".contains(&address[end - 1..end]) {
            end -= 1;
        }
        let url = &address[..end];
        let url = escape(url);
        let _ = write!(out, "<a href=\"{url}\">{url}</a>");
        rest = &address[end..];
    }
    out.push_str(&escape(rest));
    out
}

/// Pango markup for `markdown`, safe to give to a label.
pub fn to_pango(markdown: &str) -> String {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut writer = Writer::default();
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(tag) => writer.start(&tag),
            Event::End(tag) => writer.end(tag),
            Event::Text(text) => writer.text(&text),
            Event::Code(code) => {
                writer.fresh_item = false;
                writer.out.push_str("<tt>");
                writer.out.push_str(&escape(&code));
                writer.out.push_str("</tt>");
            }
            Event::Html(html) | Event::InlineHtml(html) => writer.out.push_str(&escape(&html)),
            // Todoist shows each line of a description on its own line.
            Event::SoftBreak | Event::HardBreak => {
                writer.out.push('\n');
                if writer.quotes > 0 {
                    writer.out.push_str("▎ ");
                }
            }
            Event::Rule => {
                writer.block();
                writer.out.push_str("―――――");
            }
            Event::TaskListMarker(done) => {
                if writer.out.ends_with("• ") {
                    writer.out.truncate(writer.out.len() - "• ".len());
                }
                writer.out.push_str(if done { "☑ " } else { "☐ " });
            }
            _ => {}
        }
    }
    writer.out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks the markup as a label would: the label takes links out before Pango reads the rest.
    fn valid(markup: &str) -> String {
        let mut pango = markup.replace("</a>", "");
        while let Some(start) = pango.find("<a href=") {
            let end = start + pango[start..].find('>').expect("a closed link tag");
            pango.replace_range(start..=end, "");
        }
        relm4::gtk::pango::parse_markup(&pango, '\0').expect("valid Pango markup");
        markup.to_owned()
    }

    #[test]
    fn emphasis_code_and_links() {
        assert_eq!(
            valid(&to_pango(
                "**gras**, *italique*, `code` et [lien](https://a.b/?x=1&y=2)"
            )),
            "<b>gras</b>, <i>italique</i>, <tt>code</tt> et \
             <a href=\"https://a.b/?x=1&amp;y=2\">lien</a>"
        );
    }

    #[test]
    fn bare_addresses_become_links_without_their_punctuation() {
        assert_eq!(
            valid(&to_pango("voir https://example.com/a.")),
            "voir <a href=\"https://example.com/a\">https://example.com/a</a>."
        );
    }

    #[test]
    fn lists_and_task_lists() {
        assert_eq!(
            valid(&to_pango(
                "Avant\n\n- un\n- deux\n  1. a\n  2. b\n\n- [x] fait\n- [ ] à faire"
            )),
            "Avant\n\n• un\n• deux\n    1. a\n    2. b\n☑ fait\n☐ à faire"
        );
    }

    #[test]
    fn lines_stay_lines_and_markup_is_escaped() {
        assert_eq!(
            valid(&to_pango("a < b\nc & d\n\n# Titre\n\n<b>pas du gras</b>")),
            "a &lt; b\nc &amp; d\n\n<span size=\"x-large\" weight=\"bold\">Titre</span>\n\n\
             &lt;b&gt;pas du gras&lt;/b&gt;"
        );
    }

    #[test]
    fn code_blocks_and_quotes_are_valid() {
        valid(&to_pango(
            "```\nfn main() {}\n```\n\n> cité\n> encore\n\n---\n\n![img](https://x/y.png)",
        ));
    }
}
