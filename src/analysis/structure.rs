// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Structural queries reparse on demand rather than enlarging workspace snapshots.
use super::{span_range, spans, Document};
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
