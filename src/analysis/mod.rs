// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editor analysis directly over parser events; no dependency on an LSP runtime.
//! Snapshots retain source, analysis records and top-level parsing boundaries.
//! Completion reparses a local block rather than retaining the event stream.
//! Ranges are half-open UTF-8 byte offsets.

pub mod links;
pub mod moves;
pub mod references;
pub mod rename;
pub mod text;
use crate::parser::{parse_events, Event};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};
use text::{source_range, LineIndex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Link,
    Footnote,
    Anchor,
    Heading,
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub kind: SymbolKind,
    pub key: String,
    pub name_range: Range<usize>,
    pub full_range: Range<usize>,
    pub destination: String,
}

#[derive(Debug, Clone)]
pub struct Reference {
    pub kind: SymbolKind,
    pub key: String,
    pub name_range: Range<usize>,
    pub full_range: Range<usize>,
    pub target: Option<usize>,
}

impl Reference {
    /// Expand a collapsed link's empty label instead of changing visible text.
    pub(crate) fn label_range(&self) -> Range<usize> {
        if self.kind == SymbolKind::Link && self.name_range.end < self.full_range.end - 1 {
            self.full_range.end - 1..self.full_range.end - 1
        } else {
            self.name_range.clone()
        }
    }
}

/// A source link with its decoded destination, before resolving a document URI.
#[derive(Debug, Clone)]
pub struct Link {
    pub range: Range<usize>,
    /// Source destination text, or the reference label for a reference-style use.
    pub destination_range: Range<usize>,
    pub destination: String,
}

#[derive(Debug, Clone)]
pub struct Heading {
    pub id: String,
    pub level: usize,
    pub title: String,
    pub range: Range<usize>,
    pub selection: Range<usize>,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub range: Range<usize>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionContext {
    pub kind: CompletionKind,
    pub replace: Range<usize>,
    pub prefix: String,
    pub suffix: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Destination,
    Link,
    Footnote,
    Anchor,
    Template,
}

/// Immutable analysis of one complete source version, without an AST.
///
/// Retain block boundaries instead of parser events: events and their owned
/// annotation strings dominate long-lived memory in markup-heavy workspaces.
/// Completion can recover its context by reparsing one top-level block. This
/// trades query allocations for smaller snapshots; one enormous block still
/// requires a large temporary parse. Measurements and historical comparisons
/// are in docs/memory.dj. Construction still collects the full event stream.
pub struct Document {
    pub source: String,
    block_starts: Box<[usize]>,
    pub lines: LineIndex,
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
    pub links: Vec<Link>,
    pub headings: Vec<Heading>,
    pub diagnostics: Vec<Diagnostic>,
    reverse: HashMap<usize, Vec<usize>>,
}

#[derive(Debug)]
struct Span {
    tag: String,
    open: usize,
    close: usize,
}

fn base(annot: &str) -> &str {
    annot.split('|').next().unwrap_or(annot)
}

fn spans(events: &[Event]) -> Vec<Span> {
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut result = Vec::new();
    for (i, e) in events.iter().enumerate() {
        let annot = base(&e.annot);
        if let Some(tag) = annot.strip_prefix('+') {
            stack.push((tag.to_string(), i));
        } else if let Some(tag) = annot.strip_prefix('-') {
            if let Some(idx) = stack.iter().rposition(|(name, _)| name == tag) {
                let (tag, open) = stack.remove(idx);
                result.push(Span {
                    tag,
                    open,
                    close: i,
                });
            }
        }
    }
    result
}

/// djot.js label normalization preserves case and collapses ASCII whitespace.
pub fn normalize_label(s: &str) -> String {
    s.trim()
        .split([' ', '\t', '\r', '\n'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn plain_text(source: &str, events: &[Event]) -> String {
    let mut text = String::new();
    let mut hidden = 0usize;
    for e in events {
        let annot = base(&e.annot);
        if matches!(
            annot,
            "+destination" | "+reference" | "+attributes" | "+block_attributes"
        ) {
            hidden += 1;
        }
        if hidden > 0 {
            if matches!(
                annot,
                "-destination" | "-reference" | "-attributes" | "-block_attributes"
            ) {
                hidden -= 1;
            }
            continue;
        }
        match annot {
            "str" => text.push_str(&source[source_range(source, e.startpos, e.endpos)]),
            "soft_break" | "hard_break" => text.push('\n'),
            "left_single_quote" | "right_single_quote" => text.push('\''),
            "left_double_quote" | "right_double_quote" => text.push('"'),
            "ellipses" => text.push_str("..."),
            "en_dash" => text.push_str("--"),
            "em_dash" => text.push_str("---"),
            _ => {}
        }
    }
    text
}

fn identifier(title: &str, used: &HashSet<String>) -> String {
    let base = title
        .split(|c: char| c.is_whitespace() || "][~!@#$%^&*(){}`,.<>\\|=+/?".contains(c))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let mut id = base.clone();
    let mut n = 0;
    while id.is_empty() || used.contains(&id) {
        n += 1;
        id = format!("{}-{n}", if base.is_empty() { "s" } else { &base });
    }
    id
}

impl Document {
    pub fn new(source: String) -> Self {
        let events: Vec<_> = parse_events(&source).collect();
        let block_starts = block_starts(&source, &events);
        let lines = LineIndex::new(&source);
        let mut doc = Self {
            source,
            block_starts,
            lines,
            definitions: vec![],
            references: vec![],
            links: vec![],
            headings: vec![],
            diagnostics: vec![],
            reverse: HashMap::new(),
        };
        doc.analyze(&events);
        doc
    }

    fn analyze(&mut self, events: &[Event]) {
        let spans = spans(events);
        let mut explicit_ids: HashMap<usize, String> = HashMap::new();
        // Resolve attribute ownership before deriving heading IDs. Ordinary
        // block attributes attach only across adjacent attribute groups.
        for (i, _) in events.iter().enumerate().filter(|(_, e)| e.annot == "id") {
            let name_range = event_range(&self.source, events, i);
            let key = self.source[name_range.clone()].to_string();
            let attr = spans.iter().find(|s| {
                s.open < i
                    && i < s.close
                    && matches!(s.tag.as_str(), "attributes" | "block_attributes")
            });
            let owner = attr.and_then(|attr| {
                if attr.tag == "block_attributes" {
                    events
                        .iter()
                        .enumerate()
                        .skip(attr.close + 1)
                        .take_while(|(_, e)| e.annot != "blankline")
                        .find(|(_, e)| {
                            matches!(
                                base(&e.annot),
                                "+heading"
                                    | "+para"
                                    | "+div"
                                    | "+code_block"
                                    | "+block_quote"
                                    | "+list"
                                    | "+table"
                                    | "+footnote"
                                    | "+reference_definition"
                                    | "thematic_break"
                            )
                        })
                        .map(|(i, _)| i)
                } else {
                    spans
                        .iter()
                        .find(|s| s.close + 1 == attr.open && s.tag != "attributes")
                        .map(|s| s.open)
                }
            });
            if attr.is_some_and(|s| s.tag == "block_attributes") && owner.is_none() {
                continue;
            }
            if let Some(owner) = owner {
                explicit_ids.insert(owner, key.clone());
            }
            let bare_word = attr
                .filter(|a| a.tag == "attributes")
                .and_then(|a| a.open.checked_sub(1))
                .filter(|&i| events[i].annot == "str")
                .and_then(|i| {
                    let range = event_range(&self.source, events, i);
                    let text = &self.source[range.clone()];
                    if text.chars().last().is_some_and(char::is_whitespace) {
                        return None;
                    }
                    let start = text.rfind(char::is_whitespace).map_or(range.start, |i| {
                        range.start + i + text[i..].chars().next().unwrap().len_utf8()
                    });
                    Some(start..range.end)
                });
            if owner.is_none() && bare_word.is_none() {
                continue;
            }
            let full_range = owner
                .and_then(|o| spans.iter().find(|s| s.open == o))
                .map(|s| span_range(&self.source, events, s))
                .or(bare_word)
                .unwrap_or_else(|| name_range.clone());
            self.definitions.push(Definition {
                kind: SymbolKind::Anchor,
                key: key.clone(),
                name_range,
                full_range,
                destination: format!("#{key}"),
            });
        }
        let links: HashMap<_, _> = spans
            .iter()
            .filter(|s| matches!(s.tag.as_str(), "linktext" | "imagetext"))
            .map(|s| (s.close + 1, s))
            .collect();
        let mut ordered: Vec<_> = spans.iter().collect();
        ordered.sort_by_key(|s| s.close);
        let mut used = HashSet::new();
        let mut processed_id = 0;
        for span in ordered {
            for e in &events[processed_id..=span.close] {
                if e.annot == "id" {
                    used.insert(
                        self.source[source_range(&self.source, e.startpos, e.endpos)].to_string(),
                    );
                }
            }
            processed_id = span.close + 1;
            let range = span_range(&self.source, events, span);
            let inner = &events[span.open + 1..span.close];
            match span.tag.as_str() {
                "reference_definition" => {
                    if let Some(key_event) = inner.iter().find(|e| e.annot == "reference_key") {
                        let key_range = source_range(
                            &self.source,
                            key_event.startpos + 1,
                            key_event.endpos.saturating_sub(1),
                        );
                        let key = normalize_label(&self.source[key_range.clone()]);
                        let destination = inner
                            .iter()
                            .filter(|e| e.annot == "reference_value")
                            .map(|e| &self.source[source_range(&self.source, e.startpos, e.endpos)])
                            .collect::<String>();
                        if !key.is_empty() {
                            let values: Vec<_> = inner
                                .iter()
                                .filter(|e| e.annot == "reference_value")
                                .collect();
                            if let (Some(first), Some(last)) = (values.first(), values.last()) {
                                self.links.push(Link {
                                    range: source_range(&self.source, first.startpos, last.endpos),
                                    destination_range: source_range(
                                        &self.source,
                                        first.startpos,
                                        last.endpos,
                                    ),
                                    destination: destination.clone(),
                                });
                            }
                            self.definitions.push(Definition {
                                kind: SymbolKind::Link,
                                key,
                                name_range: key_range,
                                full_range: range,
                                destination,
                            });
                        }
                    }
                }
                "footnote" => {
                    if let Some(e) = inner.iter().find(|e| e.annot == "note_label") {
                        let name_range = source_range(&self.source, e.startpos, e.endpos);
                        self.definitions.push(Definition {
                            kind: SymbolKind::Footnote,
                            key: normalize_label(&self.source[name_range.clone()]),
                            name_range,
                            full_range: range,
                            destination: String::new(),
                        });
                    }
                }
                "heading" => {
                    let title = plain_text(&self.source, inner).trim().to_string();
                    let id = explicit_ids
                        .get(&span.open)
                        .cloned()
                        .unwrap_or_else(|| identifier(&title, &used));
                    used.insert(id.clone());
                    let selection = inner
                        .first()
                        .map(|e| {
                            let start = source_range(&self.source, e.startpos, e.endpos).start;
                            let end = range.end.max(start);
                            let trimmed = self.source[start..end].trim_end();
                            start..start + trimmed.len()
                        })
                        .unwrap_or_else(|| range.clone());
                    if !explicit_ids.contains_key(&span.open) {
                        self.definitions.push(Definition {
                            kind: SymbolKind::Anchor,
                            key: id.clone(),
                            name_range: selection.clone(),
                            full_range: range.clone(),
                            destination: format!("#{id}"),
                        });
                    }
                    self.definitions.push(Definition {
                        kind: SymbolKind::Heading,
                        key: normalize_label(&title),
                        name_range: selection.clone(),
                        full_range: range.clone(),
                        destination: format!("#{id}"),
                    });
                    let open = &events[span.open];
                    self.headings.push(Heading {
                        id,
                        level: open.endpos + 1 - open.startpos,
                        title,
                        range,
                        selection,
                        parent: None,
                    });
                }
                "reference" | "destination" => {
                    let text = plain_text(&self.source, inner);
                    let is_reference = span.tag == "reference";
                    let link = links.get(&span.open);
                    let Some(link) = link else {
                        continue;
                    };
                    let full_range = event_range(&self.source, events, link.open).start..range.end;
                    if !is_reference {
                        let destination = text.replace(['\r', '\n'], "");
                        self.links.push(Link {
                            range: full_range.clone(),
                            destination_range: event_range(&self.source, events, span.open).end
                                ..event_range(&self.source, events, span.close).start,
                            destination: destination.clone(),
                        });
                        if !destination.starts_with('#') || destination == "#" {
                            continue;
                        }
                    }
                    let name_start = event_range(&self.source, events, span.open).end;
                    let name_end = event_range(&self.source, events, span.close).start;
                    let mut name_range = name_start.min(name_end)..name_end;
                    let key = if is_reference && text.is_empty() {
                        name_range = event_range(&self.source, events, link.open).end
                            ..event_range(&self.source, events, link.close).start;
                        normalize_label(&plain_text(
                            &self.source,
                            &events[link.open + 1..link.close],
                        ))
                    } else if is_reference {
                        normalize_label(&text)
                    } else {
                        name_range.start = (name_range.start + 1).min(name_range.end);
                        percent_encoding::percent_decode_str(&text[1..])
                            .decode_utf8()
                            .map(|key| key.into_owned())
                            .unwrap_or_else(|_| text[1..].to_string())
                    };
                    self.references.push(Reference {
                        kind: if is_reference {
                            SymbolKind::Link
                        } else {
                            SymbolKind::Anchor
                        },
                        key,
                        name_range,
                        full_range,
                        target: None,
                    });
                }
                _ => {}
            }
        }
        for e in events.iter().filter(|e| e.annot == "footnote_reference") {
            let full_range = source_range(&self.source, e.startpos, e.endpos);
            let name_range = (full_range.start + 2).min(full_range.end)
                ..full_range.end.saturating_sub(1).max(full_range.start + 2);
            self.references.push(Reference {
                kind: SymbolKind::Footnote,
                key: normalize_label(&self.source[name_range.clone()]),
                name_range,
                full_range,
                target: None,
            });
        }
        self.references.sort_by_key(|r| r.name_range.start);
        self.headings.sort_by_key(|h| h.range.start);
        let mut parents: Vec<usize> = vec![];
        for i in 0..self.headings.len() {
            while parents
                .last()
                .is_some_and(|&p| self.headings[p].level >= self.headings[i].level)
            {
                parents.pop();
            }
            self.headings[i].parent = parents.last().copied();
            parents.push(i);
        }
        let mut lookup = HashMap::new();
        // Implicit heading labels use the first heading; explicit definitions
        // override implicit labels and later explicit definitions win.
        for (i, d) in self
            .definitions
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == SymbolKind::Heading)
        {
            lookup.entry((SymbolKind::Link, d.key.clone())).or_insert(i);
        }
        let mut seen = HashSet::new();
        for (i, d) in self
            .definitions
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind != SymbolKind::Heading)
        {
            let key = (d.kind, d.key.clone());
            if !seen.insert(key.clone()) {
                self.diagnostics.push(Diagnostic {
                    range: d.name_range.clone(),
                    severity: Severity::Warning,
                    message: format!("Duplicate definition: {}", d.key),
                });
            }
            lookup.insert(key, i);
        }
        for (i, r) in self.references.iter_mut().enumerate() {
            r.target = lookup.get(&(r.kind, r.key.clone())).copied();
            if let Some(target) = r.target {
                self.reverse.entry(target).or_default().push(i);
            } else {
                self.diagnostics.push(Diagnostic {
                    range: r.name_range.clone(),
                    severity: Severity::Error,
                    message: format!("Unresolved reference: {}", r.key),
                });
            }
        }
        // Resolve reference-style links using exactly the same precedence as
        // definition navigation; unresolved references are not clickable.
        for reference in &self.references {
            if reference.kind == SymbolKind::Link {
                if let Some(target) = reference.target {
                    self.links.push(Link {
                        range: reference.full_range.clone(),
                        destination_range: reference.name_range.clone(),
                        destination: self.definitions[target].destination.clone(),
                    });
                }
            }
        }
        self.links.sort_by_key(|link| link.range.start);
    }

    pub fn target_at(&self, offset: usize) -> Option<usize> {
        self.references
            .iter()
            .find(|r| r.name_range.contains(&offset))
            .and_then(|r| r.target)
            .or_else(|| {
                self.definitions
                    .iter()
                    .position(|d| d.name_range.contains(&offset))
            })
    }

    pub fn references_to(&self, definition: usize) -> impl Iterator<Item = &Reference> {
        let selected = &self.definitions[definition];
        self.definitions
            .iter()
            .enumerate()
            .filter(move |(i, d)| {
                *i == definition
                    || (matches!(selected.kind, SymbolKind::Heading | SymbolKind::Anchor)
                        && matches!(d.kind, SymbolKind::Heading | SymbolKind::Anchor)
                        && d.full_range == selected.full_range)
            })
            .flat_map(|(i, _)| self.reverse.get(&i).into_iter().flatten())
            .map(|&i| &self.references[i])
    }

    /// Reparse only the top-level block containing the cursor. Positions are
    /// translated into the original source so document EOF remains distinct
    /// from an artificial end of the local parsing range.
    pub fn completion_context(&self, offset: usize) -> Option<CompletionContext> {
        if offset > self.source.len() || !self.source.is_char_boundary(offset) {
            return None;
        }
        let index = self
            .block_starts
            .partition_point(|&start| start < offset)
            .saturating_sub(1);
        let start = self.block_starts[index];
        let end = self
            .block_starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.source.len());
        let events: Vec<_> = parse_events(&self.source[start..end])
            .map(|mut event| {
                event.startpos += start;
                event.endpos += start;
                event
            })
            .collect();
        completion_context_in(&self.source, &events, offset)
    }
}

fn event_range(source: &str, events: &[Event], i: usize) -> Range<usize> {
    let e = &events[i];
    source_range(source, e.startpos, e.endpos)
}

fn span_range(source: &str, events: &[Event], span: &Span) -> Range<usize> {
    let start = event_range(source, events, span.open).start;
    // Most block close events are boundary markers, not source characters.
    let end = if matches!(
        span.tag.as_str(),
        "para"
            | "heading"
            | "footnote"
            | "reference_definition"
            | "list"
            | "list_item"
            | "block_quote"
            | "table"
    ) {
        events[span.close].startpos.min(source.len())
    } else {
        event_range(source, events, span.close).end
    };
    start..end.max(start)
}

fn completion_context_in(
    source: &str,
    events: &[Event],
    offset: usize,
) -> Option<CompletionContext> {
    if offset > source.len() || !source.is_char_boundary(offset) {
        return None;
    }
    let spans = spans(events);
    if spans.iter().any(|s| {
        matches!(
            s.tag.as_str(),
            "code_block"
                | "verbatim"
                | "inline_math"
                | "display_math"
                | "url"
                | "email"
                | "attributes"
                | "block_attributes"
        ) && {
            let range = span_range(source, events, s);
            range.start < offset
                && (offset < range.end
                    || (offset == range.end
                        && (s.tag == "code_block" && events[s.close].startpos >= source.len())))
        }
    }) {
        return None;
    }
    let start = spans
        .iter()
        .filter(|s| {
            matches!(s.tag.as_str(), "para" | "heading" | "caption" | "cell") && {
                let range = span_range(source, events, s);
                range.start <= offset && offset <= range.end
            }
        })
        .map(|s| event_range(source, events, s.open).start)
        .max()
        .unwrap_or_else(|| source[..offset].rfind('\n').map_or(0, |i| i + 1));
    let before = &source[start..offset];
    let bracket = before.rfind('[')?;
    let absolute = start + bracket;
    if spans.iter().any(|s| {
        matches!(
            s.tag.as_str(),
            "code_block"
                | "verbatim"
                | "inline_math"
                | "display_math"
                | "url"
                | "email"
                | "attributes"
                | "block_attributes"
        ) && span_range(source, events, s).contains(&absolute)
    }) {
        return None;
    }
    if escaped(source, absolute) {
        return None;
    }
    let tail = &before[bracket + 1..];
    let after = &source[offset..];
    let anchor = before.rfind("](#").filter(|&i| i >= bracket);
    let (kind, prefix, name_start, close) = if let Some(anchor) = anchor {
        (
            CompletionKind::Anchor,
            &before[anchor + 3..],
            start + anchor + 3,
            ')',
        )
    } else if let Some(destination) = before.rfind("](").filter(|&i| i >= bracket) {
        (
            CompletionKind::Destination,
            &before[destination + 2..],
            start + destination + 2,
            ')',
        )
    } else if let Some(prefix) = tail.strip_prefix('^') {
        (CompletionKind::Footnote, prefix, absolute + 2, ']')
    } else if bracket > 0 && before.as_bytes()[bracket - 1] == b']' {
        (CompletionKind::Link, tail, absolute + 1, ']')
    } else if tail.is_empty() {
        (CompletionKind::Template, tail, absolute, ']')
    } else {
        return None;
    };
    if prefix.contains(['[', ']', '(', ')', '\r', '\n']) {
        return None;
    }
    let existing_end = after.find([close, '\n', '\r']);
    // Replace the remainder only when a closing delimiter is present on
    // this line; otherwise do not swallow unrelated text after the cursor.
    let replace_end = existing_end
        .filter(|&i| after.as_bytes()[i] == close as u8)
        .map(|i| offset + i + 1)
        .unwrap_or(offset);
    let replace_end = if matches!(kind, CompletionKind::Template | CompletionKind::Link)
        && after.starts_with("]()")
    {
        offset + 3
    } else if kind == CompletionKind::Link && after.starts_with(')') {
        offset + 1
    } else if kind == CompletionKind::Template {
        if after.starts_with(']') {
            offset + 1
        } else {
            offset
        }
    } else {
        replace_end
    };
    Some(CompletionContext {
        kind,
        replace: name_start..replace_end,
        prefix: prefix.to_string(),
        suffix: if matches!(kind, CompletionKind::Anchor | CompletionKind::Destination) {
            ")"
        } else {
            "]"
        },
    })
}

/// Partition source at top-level block starts, retaining intervening whitespace.
/// Block attributes belong to the following block and share its parsing range.
/// Splitting at nested blocks would lose their enclosing list/quote context;
/// dropping preceding attributes would allow completion inside attribute text.
fn block_starts(source: &str, events: &[Event]) -> Box<[usize]> {
    let mut starts = vec![0];
    let mut depth = 0usize;
    let mut attributes_pending = false;
    for event in events {
        let tag = base(&event.annot);
        let opens = tag.starts_with('+');
        if depth == 0 && (opens || tag == "thematic_break") {
            let start = source[..event.startpos.min(source.len())]
                .rfind('\n')
                .map_or(0, |i| i + 1);
            if !attributes_pending && start > *starts.last().unwrap() && start < source.len() {
                starts.push(start);
            }
            attributes_pending = tag == "+block_attributes";
        }
        if opens {
            depth += 1;
        } else if tag.starts_with('-') {
            depth = depth.saturating_sub(1);
        }
    }
    starts.into_boxed_slice()
}

fn escaped(source: &str, offset: usize) -> bool {
    source[..offset]
        .bytes()
        .rev()
        .take_while(|&b| b == b'\\')
        .count()
        % 2
        == 1
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    fn compare(source: String, name: &str) {
        let events: Vec<_> = parse_events(&source).collect();
        let doc = Document::new(source);
        for offset in 0..=doc.source.len() + 1 {
            assert_eq!(
                doc.completion_context(offset),
                completion_context_in(&doc.source, &events, offset),
                "local/full completion mismatch in {name} at byte {offset}: {:?}",
                doc.source
            );
        }
    }

    #[test]
    fn top_level_parsing_boundaries() {
        let source = "first\n\n{#id}\n{.class}\n- item\n\n  > nested\n\nnext\n";
        let doc = Document::new(source.to_owned());
        assert_eq!(
            &*doc.block_starts,
            &[
                0,
                source.find("{#id}").unwrap(),
                source.find("next").unwrap()
            ]
        );
    }

    #[test]
    fn local_completion_context_boundaries() {
        for template in [
            "before\n\nHERE\n\nafter",
            "before\n\n{#id}\n{.class}\nHERE\n\nafter",
            "before\n\n```\nHERE\n```\n\nafter",
            "before\n\n`HERE` outside\n\nafter",
            "before\n\n$`HERE` outside\n\nafter",
            "before\n\n{key=\"HERE\"}\nparagraph\n\nafter",
            "before\n\n- first\n\n  > HERE\n\nafter",
            "before\n\n[^note]: HERE\n\nafter",
            "before\n\n::: note\nHERE\n:::\n\nafter",
            "before\n\n| HERE | other |\n\nafter",
            "before\n\n```\ncode\n```\nHERE",
        ] {
            for prefix in ["[", "[^fo", "[text][lab", "[text](#an", "[text](dest"] {
                let source = template.replace("HERE", prefix);
                compare(source.clone(), "completion context boundaries");
                compare(
                    source.replace('\n', "\r\n"),
                    "CRLF completion context boundaries",
                );
            }
        }
    }

    #[test]
    fn local_completion_matches_full_parse() {
        for entry in glob::glob("third_party/djot.js/test/*.test").unwrap() {
            let path = entry.unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            let mut lines = content.lines();
            let mut case = 0;
            while let Some(line) = lines.next() {
                if !line.starts_with("```") {
                    continue;
                }
                let mut source = String::new();
                for line in lines.by_ref() {
                    if line == "." || line == "!" {
                        break;
                    }
                    source.push_str(line);
                    source.push('\n');
                }
                for line in lines.by_ref() {
                    if line.starts_with("```") {
                        break;
                    }
                }
                case += 1;
                compare(source, &format!("{}:{case}", path.display()));
            }
        }
        for entry in glob::glob("tests/*.in").unwrap() {
            let path = entry.unwrap();
            compare(
                std::fs::read_to_string(&path).unwrap(),
                &path.display().to_string(),
            );
        }
        for source in [
            "[",
            "[^fo",
            "[text][lab",
            "[text](#an",
            "```\n[^foo",
            "> [^fo",
            "- one\n\n  [^fo",
            "::: note\n[^fo",
            "{key=\"unfinished",
            "text `[^fo",
            "# heading\n[^fo",
            "| [^fo | other |\n",
            "{#id}\n{.class}\n[^fo\n\nnext",
            "```\ncode\n```\n[^fo",
            "  - item\n\n  - [^fo\n\nnext",
            "[中文][标\n\n[^脚",
            "\r\n{#id}\r\n[^fo\r\n\r\nnext",
            "\n\n",
            "",
        ] {
            compare(source.to_owned(), "incomplete/boundary");
        }
    }
}
