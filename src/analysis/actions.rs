// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Explicit, document-local removal of confirmed broken ordinary links.
use super::{
    event_range,
    links::{Resolution, Resolver},
    rename::Edit,
    span_range, spans, Document, SymbolKind,
};
use crate::parser::{parse_events, Event};
use std::ops::Range;

fn intersects(range: &Range<usize>, selection: &Range<usize>) -> bool {
    if selection.is_empty() {
        range.contains(&selection.start)
    } else {
        range.start < selection.end && selection.start < range.end
    }
}

// Compare semantic event content, coalescing adjacent literal text after the
// delimiters disappear. Neutral spans and escapes do not change visible text.
// This catches accidental block syntax, new references and interactions with
// adjacent inline delimiters. A neutral span is the fallback for such cases.
fn signature(source: &str, events: impl IntoIterator<Item = Event>) -> Vec<(String, String)> {
    let mut result: Vec<(String, String)> = Vec::new();
    for event in events {
        let tag = event.annot.split('|').next().unwrap();
        if matches!(
            tag,
            "escape" | "+span" | "-span" | "+attributes" | "-attributes"
        ) {
            continue;
        }
        let value = if tag.starts_with(['+', '-']) || tag == "blankline" {
            String::new()
        } else {
            source[super::text::source_range(source, event.startpos, event.endpos)].to_string()
        };
        if tag == "str" && result.last().is_some_and(|(tag, _)| tag == "str") {
            result.last_mut().unwrap().1.push_str(&value);
        } else {
            result.push((event.annot, value));
        }
    }
    result
}

impl Document {
    /// Return one independent edit per broken link overlapping the request.
    /// Filesystem resolution must run on a blocking worker, as for diagnostics.
    pub fn remove_broken_links(
        &self,
        uri: &str,
        selection: Range<usize>,
        resolver: &mut Resolver,
    ) -> Vec<Edit> {
        let events: Vec<_> = parse_events(&self.source).collect();
        let spans = spans(&events);
        let mut edits = Vec::new();
        for label in spans.iter().filter(|span| span.tag == "linktext") {
            let Some(destination) = spans.iter().find(|span| {
                span.open == label.close + 1
                    && matches!(span.tag.as_str(), "destination" | "reference")
            }) else {
                continue;
            };
            let full = event_range(&self.source, &events, label.open).start
                ..span_range(&self.source, &events, destination).end;
            if !intersects(&full, &selection) {
                continue;
            }
            let reference = self.references.iter().find(|reference| {
                reference.full_range == full && reference.kind == SymbolKind::Link
            });
            let broken = if reference.is_some_and(|reference| reference.target.is_none()) {
                true
            } else if let Some(link) = self.links.iter().find(|link| link.range == full) {
                matches!(
                    resolver.resolve(uri, &link.destination),
                    Resolution::MissingFile(_) | Resolution::MissingAnchor(_)
                )
            } else {
                false
            };
            if !broken {
                continue;
            }
            let start = event_range(&self.source, &events, label.open).end;
            let end = event_range(&self.source, &events, label.close).start;
            let mut text = self.source[start..end].to_string();
            // Escape literal punctuation while retaining code, math and inline
            // formatting. Remove an existing escape together with its character.
            let mut protected = 0usize;
            let mut replacements = Vec::new();
            for i in label.open + 1..label.close {
                let tag = events[i].annot.as_str();
                match tag {
                    "+verbatim" | "+inline_math" | "+display_math" => protected += 1,
                    "-verbatim" | "-inline_math" | "-display_math" => protected -= 1,
                    "str" if protected == 0 => {
                        let mut bytes = event_range(&self.source, &events, i);
                        let literal = &self.source[bytes.clone()];
                        let escaped: String = literal
                            .chars()
                            .flat_map(|ch| {
                                ch.is_ascii_punctuation()
                                    .then_some('\\')
                                    .into_iter()
                                    .chain(std::iter::once(ch))
                            })
                            .collect();
                        if events[i - 1].annot == "escape" {
                            bytes.start = events[i - 1].startpos;
                        }
                        replacements.push((bytes, escaped));
                    }
                    _ => {}
                }
            }
            for (bytes, escaped) in replacements.into_iter().rev() {
                text.replace_range(bytes.start - start..bytes.end - start, &escaped);
            }
            let expected = signature(
                &self.source,
                events
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        *i != label.open
                            && *i != label.close
                            && !(*i >= destination.open && *i <= destination.close)
                    })
                    .map(|(_, event)| event.clone()),
            );
            let attributes = events
                .get(destination.close + 1)
                .is_some_and(|event| event.annot == "+attributes");
            let candidates = if attributes {
                vec![format!("[{text}]")]
            } else {
                vec![text.clone(), format!("[{text}]{{}}")]
            };
            for text in candidates {
                let mut source = self.source.clone();
                source.replace_range(full.clone(), &text);
                if signature(&source, parse_events(&source)) == expected {
                    edits.push(Edit { range: full, text });
                    break;
                }
            }
        }
        edits
    }
}
