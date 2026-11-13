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
