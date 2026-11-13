// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Structural queries reparse on demand rather than enlarging workspace snapshots.
use super::{event_range, span_range, spans, Document};
use crate::parser::parse_events;
use std::ops::Range;

impl Document {
    fn sections(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        self.headings.iter().enumerate().map(|(i, heading)| {
            let end = self.headings[i + 1..]
                .iter()
                .find(|next| next.level <= heading.level)
                .map_or(self.source.len(), |next| next.range.start);
            heading.range.start..end
        })
    }

    /// Return strictly expanding syntax ranges for each cursor, smallest first.
    /// Parsing is shared by all positions in one request.
    pub fn selection_ranges(&self, offsets: &[usize]) -> Vec<Vec<Range<usize>>> {
        let events: Vec<_> = parse_events(&self.source).collect();
        let mut ranges = Vec::new();
        ranges.push(0..self.source.len());
        for span in spans(&events) {
            ranges.push(span_range(&self.source, &events, &span));
            if matches!(
                span.tag.as_str(),
                "linktext"
                    | "imagetext"
                    | "destination"
                    | "reference"
                    | "verbatim"
                    | "strong"
                    | "emph"
            ) {
                let start = event_range(&self.source, &events, span.open).end;
                let end = event_range(&self.source, &events, span.close).start;
                if start < end {
                    ranges.push(start..end);
                }
            }
        }
        ranges.extend(
            events
                .iter()
                .filter(|e| e.annot == "str")
                .map(|e| super::text::source_range(&self.source, e.startpos, e.endpos)),
        );
        ranges.extend(self.sections());
        ranges.extend(self.links.iter().map(|link| link.range.clone()));
        ranges.extend(
            self.references
                .iter()
                .map(|reference| reference.full_range.clone()),
        );
        ranges.sort_by_key(|range| (range.len(), range.start));
        ranges.dedup();
        offsets
            .iter()
            .map(|&offset| {
                let mut chain = Vec::new();
                // Word boundaries are independent of UTF-8 byte length. Keep Djot
                // punctuation outside words so syntax ranges remain useful steps.
                let word_char = |ch: char| ch.is_alphanumeric() || ch == '_';
                let start = self.source[..offset]
                    .rfind(|ch: char| !word_char(ch))
                    .map_or(0, |i| {
                        i + self.source[i..].chars().next().unwrap().len_utf8()
                    });
                let end = self.source[offset..]
                    .find(|ch: char| !word_char(ch))
                    .map_or(self.source.len(), |i| offset + i);
                if start < end {
                    chain.push(start..end);
                }
                for candidate in &ranges {
                    if candidate.start > offset || candidate.end < offset {
                        continue;
                    }
                    if chain.last().is_none_or(|last| {
                        candidate != last
                            && candidate.start <= last.start
                            && candidate.end >= last.end
                    }) {
                        chain.push(candidate.clone());
                    }
                }
                if chain.is_empty() {
                    chain.push(0..self.source.len());
                }
                chain
            })
            .collect()
    }

    /// Fold sections and multiline containers, preserving their first line.
    /// Trailing whitespace belongs to neither the section nor the folded block.
    pub fn folding_ranges(&self) -> Vec<Range<usize>> {
        let events: Vec<_> = parse_events(&self.source).collect();
        let mut ranges: Vec<_> = spans(&events)
            .iter()
            .filter(|span| {
                matches!(
                    span.tag.as_str(),
                    "code_block"
                        | "list"
                        | "list_item"
                        | "block_quote"
                        | "div"
                        | "footnote"
                        | "table"
                )
            })
            .map(|span| span_range(&self.source, &events, span))
            .chain(self.sections())
            .map(|range| {
                let end = range.start + self.source[range.clone()].trim_end().len();
                range.start..end
            })
            .filter(|range| {
                self.lines.position(&self.source, range.start).0
                    < self.lines.position(&self.source, range.end).0
            })
            .collect();
        ranges.sort_by_key(|r| (r.start, r.end));
        ranges.dedup();
        ranges
    }
}
