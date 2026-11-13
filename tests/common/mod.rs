// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

/// Extract inputs from the pinned djot.js functional test corpus.
pub fn upstream_test_cases() -> Vec<(String, usize, String)> {
    let mut cases = Vec::new();
    for entry in glob::glob("third_party/djot.js/test/*.test").unwrap() {
        let path = entry.unwrap();
        let file_name = path.file_name().unwrap().to_str().unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        for (index, input) in parse_test_file(&content).into_iter().enumerate() {
            cases.push((file_name.to_owned(), index, input));
        }
    }
    assert!(
        !cases.is_empty(),
        "no upstream test cases found; initialize the djot.js submodule"
    );
    cases
}

/// Test cases are fenced; input and expected output are separated by `.` or `!`.
fn parse_test_file(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut cases = Vec::new();
    let mut idx = 0;

    while idx < lines.len() {
        while idx < lines.len() && !lines[idx].starts_with("```") {
            idx += 1;
        }
        if idx >= lines.len() {
            break;
        }
        let fence_len = lines[idx].chars().take_while(|&c| c == '`').count();
        let fence = "`".repeat(fence_len);
        idx += 1;

        let mut input = String::new();
        while idx < lines.len() && !matches!(lines[idx], "." | "!") {
            input.push_str(lines[idx]);
            input.push('\n');
            idx += 1;
        }
        assert!(
            idx < lines.len(),
            "upstream test case has no input separator"
        );
        idx += 1;

        // Match the opening fence length: expected output can contain shorter
        // backtick runs that are not the end of the enclosing test case.
        while idx < lines.len() && !lines[idx].starts_with(&fence) {
            idx += 1;
        }
        assert!(idx < lines.len(), "upstream test case has no closing fence");
        idx += 1;
        cases.push(input);
    }

    cases
}

pub fn discover_documents() -> Vec<std::path::PathBuf> {
    // Git includes new, non-ignored documents without traversing submodules
    // or build output. Use NUL separators so spaces and newlines in paths work.
    let output = std::process::Command::new("git")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "*.dj",
        ])
        .output()
        .expect("Git is required to discover project documentation");
    assert!(
        output.status.success(),
        "failed to discover project documentation: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paths = String::from_utf8(output.stdout).expect("document paths must be UTF-8");
    let mut documents: Vec<_> = paths
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
        .collect();
    documents.sort();
    documents.dedup();
    documents
}
