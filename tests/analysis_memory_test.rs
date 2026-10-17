// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Isolated, opt-in accounting of bytes requested from the Rust allocator.
//! Unlike RSS or mallinfo, this excludes allocator padding and freed arenas.
use djotls::analysis::Document;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
};

struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static BLOCKS: AtomicUsize = AtomicUsize::new(0);

fn add(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
}

// The wrapper delegates all storage and alignment behavior to System. Counters
// do not allocate. Update only successful allocations, including reallocations.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            add(layout.size());
            BLOCKS.fetch_add(1, Relaxed);
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            add(layout.size());
            BLOCKS.fetch_add(1, Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
        BLOCKS.fetch_sub(1, Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, old, new_size) };
        if !ptr.is_null() {
            if new_size >= old.size() {
                add(new_size - old.size());
            } else {
                LIVE.fetch_sub(old.size() - new_size, Relaxed);
            }
        }
        ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn fixture(kind: &str, bytes: usize) -> String {
    let mut source = String::new();
    let mut section = 0;
    while source.len() < bytes {
        match kind {
            "plain" => source.push_str("This paragraph contains ordinary prose about documenting a software project. It has no inline markup, links or other syntax.\n\n"),
            "mixed" => {
                source.push_str(&format!("## Section {section}\n\n"));
                for _ in 0..4 {
                    source.push_str("A document paragraph with *emphasis*, _strong text_, `code`, Unicode 中文😀 and an [external link](https://example.com/manual). This is ordinary prose for a documentation workspace. ");
                }
                source.push_str("\n\n");
            }
            "dense" => source.push_str("*a* _b_ `c` [d](https://example.com) "),
            _ => unreachable!(),
        }
        section += 1;
    }
    source
}

#[test]
#[ignore = "opt-in allocator accounting; run alone in release mode with --nocapture"]
fn analysis_memory_probe() {
    // Inputs and the fixture generator are outside each measurement. Warm each
    // input's lazy parser regexes before establishing its allocation baseline.
    let mut cases = Vec::new();
    for kind in ["plain", "mixed", "dense"] {
        for bytes in [12 * 1024, 128 * 1024, 1024 * 1024] {
            cases.push((format!("{kind}-{bytes}"), fixture(kind, bytes)));
        }
    }
    for path in ["README.dj", "docs/lsp.dj"] {
        cases.push((path.to_owned(), std::fs::read_to_string(path).unwrap()));
    }
    println!("case,source_B,retained_B,peak_B,ratio,live_blocks,query_peak_B,after_drop_delta_B");
    for (name, source) in &cases {
        drop(Document::new(source.clone()));
        let before = LIVE.load(Relaxed);
        let blocks = BLOCKS.load(Relaxed);
        PEAK.store(before, Relaxed);
        let doc = Document::new(source.clone());
        let retained = LIVE.load(Relaxed) - before;
        let peak = PEAK.load(Relaxed) - before;
        let live_blocks = BLOCKS.load(Relaxed) - blocks;
        assert_eq!(doc.source, *source);
        let query_before = LIVE.load(Relaxed);
        PEAK.store(query_before, Relaxed);
        for offset in [0, source.len() / 2, source.len()] {
            drop(doc.completion_context(offset));
        }
        let query_peak = PEAK.load(Relaxed) - query_before;
        assert_eq!(
            LIVE.load(Relaxed),
            query_before,
            "completion retains allocations: {name}"
        );
        drop(doc);
        let after_drop = LIVE.load(Relaxed) as i64 - before as i64;
        assert_eq!(
            after_drop, 0,
            "document allocations remain after drop: {name}"
        );
        println!(
            "{name},{},{retained},{peak},{:.2},{live_blocks},{query_peak},{after_drop}",
            source.len(),
            retained as f64 / source.len() as f64
        );
    }

    // Use the process probe's exact fixture, retaining all analyses together
    // without LSP or Git overhead. The external fixture stays outside counts.
    let prose = "A document paragraph with *emphasis*, _strong text_, `code`, Unicode 中文😀 and an [external link](https://example.com/manual). This is ordinary prose for a documentation workspace. ".repeat(4);
    let mut source = String::from("# Document\n\n[shared](target.dj#Shared) [label][] [^note]\n\n");
    for section in 0..16 {
        source.push_str(&format!("## Section {section}\n\n{prose}\n\n"));
    }
    source.push_str("[label]: https://example.com/guide\n[^note]: A footnote.\n");
    drop(Document::new(source.clone()));
    for count in [100, 1000] {
        let before = LIVE.load(Relaxed);
        PEAK.store(before, Relaxed);
        let docs: Vec<_> = (0..count).map(|_| Document::new(source.clone())).collect();
        let retained = LIVE.load(Relaxed) - before;
        let peak = PEAK.load(Relaxed) - before;
        assert!(docs.iter().all(|doc| doc.source == source));
        drop(docs);
        let after_drop = LIVE.load(Relaxed) as i64 - before as i64;
        assert_eq!(after_drop, 0, "corpus allocations remain after drop");
        println!("corpus_{count},source_B={},retained_B={retained},peak_B={peak},after_drop_delta_B={after_drop}", source.len() * count);
    }
}
