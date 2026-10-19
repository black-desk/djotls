// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
