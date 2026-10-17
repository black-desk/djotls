// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Workspace references over immutable snapshots, using navigation's resolver.
//! Labels and footnotes retain document scope; headings and IDs include incoming
//! links. Call on a blocking worker because out-of-index targets can require I/O.
use super::{
    links::{Resolution, Resolver},
    Document, SymbolKind,
};
use std::{collections::HashMap, ops::Range, sync::Arc};
use url::Url;

pub struct Location {
    pub uri: String,
    pub document: Arc<Document>,
    pub range: Range<usize>,
}

pub struct References {
    documents: HashMap<String, Arc<Document>>,
    resolver: Resolver,
}

fn same_document(left: &str, right: &str) -> bool {
    match (Url::parse(left), Url::parse(right)) {
        (Ok(left), Ok(right)) => match (left.to_file_path(), right.to_file_path()) {
            (Ok(left), Ok(right)) => left == right,
            _ => left == right,
        },
        _ => left == right,
    }
}

impl References {
    pub fn new(documents: HashMap<String, Arc<Document>>) -> Self {
        let resolver = Resolver::new(
            documents
                .iter()
                .map(|(uri, doc)| (uri.clone(), doc.clone())),
        );
        Self {
            documents,
            resolver,
        }
    }

    pub fn find(&mut self, uri: &str, at: usize, declaration: bool) -> Option<Vec<Location>> {
        let doc = self.documents.get(uri)?.clone();
        // A reference label goes to its local definition; the text of that link
        // goes to its destination, exactly as in definition navigation.
        let local = doc.references.iter().find(|r| r.name_range.contains(&at));
        let (target_uri, target_doc, definition) = if let Some(reference) = local {
            (uri.to_owned(), doc.clone(), reference.target?)
        } else if let Some(link) = doc.links.iter().find(|l| l.range.contains(&at)) {
            let Resolution::Target {
                uri,
                document: Some(document),
                selection,
            } = self.resolver.resolve(uri, &link.destination)
            else {
                return None;
            };
            let definition = document
                .definitions
                .iter()
                .position(|d| d.kind == SymbolKind::Anchor && d.name_range == selection)?;
            (uri.to_string(), document, definition)
        } else {
            (uri.to_owned(), doc.clone(), doc.target_at(at)?)
        };
        let target_uri = self
            .documents
            .keys()
            .find(|uri| same_document(uri, &target_uri))
            .cloned()
            .unwrap_or(target_uri);
        let selected = &target_doc.definitions[definition];
        let mut locations = Vec::new();
        let mut add = |uri: &str, document: &Arc<Document>, range: Range<usize>| {
            locations.push(Location {
                uri: uri.to_owned(),
                document: document.clone(),
                range,
            });
        };
        for reference in target_doc.references_to(definition) {
            add(&target_uri, &target_doc, reference.name_range.clone());
        }
        if matches!(selected.kind, SymbolKind::Heading | SymbolKind::Anchor) {
            // A heading and its anchor have different name ranges when the ID
            // is explicit. Compare the resolved definition, never title text.
            let anchors: Vec<_> = target_doc
                .definitions
                .iter()
                .filter(|d| d.kind == SymbolKind::Anchor && d.full_range == selected.full_range)
                .collect();
            self.documents
                .entry(target_uri.clone())
                .or_insert(target_doc.clone());
            for (source_uri, source) in &self.documents {
                for link in &source.links {
                    let Resolution::Target {
                        uri: resolved,
                        selection,
                        ..
                    } = self.resolver.resolve(source_uri, &link.destination)
                    else {
                        continue;
                    };
                    if !same_document(resolved.as_str(), &target_uri)
                        || !anchors.iter().any(|d| d.name_range == selection)
                    {
                        continue;
                    }
                    // Indirect uses select their label; direct destinations
                    // select just the fragment. Local uses are deduplicated.
                    let range = source
                        .references
                        .iter()
                        .find(|r| r.full_range == link.range)
                        .map(|r| r.name_range.clone())
                        .or_else(|| {
                            let raw = &source.source[link.destination_range.clone()];
                            let hash = raw.find('#')?;
                            Some(
                                link.destination_range.start + hash + 1..link.destination_range.end,
                            )
                        });
                    if let Some(range) = range {
                        add(source_uri, source, range);
                    }
                }
            }
        }
        if declaration {
            add(&target_uri, &target_doc, selected.name_range.clone());
        }
        locations.sort_by(|a, b| {
            (&a.uri, a.range.start, a.range.end).cmp(&(&b.uri, b.range.start, b.range.end))
        });
        locations.dedup_by(|a, b| same_document(&a.uri, &b.uri) && a.range == b.range);
        Some(locations)
    }
}
