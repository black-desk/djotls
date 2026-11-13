// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../cli.rs"]
mod cli;

fn main() -> std::io::Result<()> {
    use clap::Parser;
    let matches = cli::Cli::parse();

    colog::default_builder()
        .filter_level(match matches.verbose {
            0 => log::LevelFilter::Warn,
            1 => log::LevelFilter::Info,
            2 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        })
        .init();

    log::trace!("CLI options: {:?}", matches);

    for file in matches.input {
        log::trace!("Processing file: {}", file.display());

        let output: &mut dyn std::io::Write = match matches.inplace {
            false => {
                log::trace!("Writing to stdout");
                &mut std::io::stdout()
            }
            true => {
                let swap = file.with_extension("djotfmt.swp");

                log::trace!("Writing to swap file {}", swap.display());

                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    // Keep private input contents private while writing too.
                    options.mode(0o600);
                }
                &mut options.open(swap).expect("Cannot create swap file")
            }
        };

        log::trace!("Start render file");

        let input = std::fs::read_to_string(file.clone())?;

        let config = djotls::fmt::FmtConfig {
            max_cols: matches.columns,
        };

        let result = djotls::fmt::format(&input, &config);
        output.write_all(result.as_bytes())?;

        log::trace!("File rendered");

        if !matches.inplace {
            continue;
        }

        let swap = file.with_extension("djotfmt.swp");
        assert!(swap.exists(), "Swap file does not exist");
        // Copy permissions after writing, which can clear special mode bits.
        std::fs::set_permissions(&swap, std::fs::metadata(&file)?.permissions())?;

        log::trace!(
            "Renaming swap file {} to {}",
            swap.display(),
            file.display()
        );

        std::fs::rename(file.with_extension("djotfmt.swp"), file)?;
    }

    Ok(())
}
