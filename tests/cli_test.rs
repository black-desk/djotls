// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(unix)]
#[test]
fn inplace_formatting_preserves_file_permissions() {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let fixture =
        Fixture(std::env::temp_dir().join(format!("djotfmt-permissions-{}", std::process::id())));
    fs::create_dir(&fixture.0).unwrap();
    for mode in [0o600, 0o640, 0o755, 0o444, 0o4750] {
        for source in ["word   word\n", "word word\n"] {
            let path = fixture.0.join("private.dj");
            // Recreate read-only inputs rather than trying to overwrite them.
            if path.exists() {
                fs::remove_file(&path).unwrap();
            }
            fs::write(&path, source).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_djotfmt"))
                .arg("-i")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), "word word\n");
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
                mode,
                "permissions changed for {mode:o}, input {source:?}"
            );
            assert!(!path.with_extension("djotfmt.swp").exists());
        }
    }
}

#[test]
fn check_stdin_exit_codes_width_and_conflicting_options() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    for (source, args, code) in [
        ("word word\n", vec!["--check"], 0),
        ("word   word\n", vec!["--check"], 1),
        ("word word\n", vec!["--check", "-c", "4"], 1),
        ("word\n", vec!["--check", "-i"], 2),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_djotfmt"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // clap can reject conflicting arguments before reading stdin.
        let _ = child.stdin.take().unwrap().write_all(source.as_bytes());
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn check_reports_all_unformatted_files_without_writing() {
    use std::{fs, process::Command};
    let dir = std::env::temp_dir().join(format!("djotfmt-check-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let paths: Vec<_> = ["clean.dj", "first.dj", "second.dj"]
        .map(|name| dir.join(name))
        .into();
    for (i, path) in paths.iter().enumerate() {
        fs::write(
            path,
            if i == 0 {
                "word word\n"
            } else {
                "word   word\n"
            },
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_djotfmt"))
        .arg("--check")
        .args(&paths)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let errors = String::from_utf8(output.stderr).unwrap();
    assert!(!errors.contains("clean.dj"));
    for path in &paths[1..] {
        assert!(errors.contains(path.to_str().unwrap()));
        assert_eq!(fs::read_to_string(path).unwrap(), "word   word\n");
        assert!(!path.with_extension("djotfmt.swp").exists());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_djotfmt"))
        .arg("--check")
        .arg(dir.join("missing.dj"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    fs::remove_dir_all(dir).unwrap();
}
