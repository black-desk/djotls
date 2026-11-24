// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

const PUNCTUATION: &[char] = &[' ', '\n', '\t', ',', ';', '?', '/'];

/// A small deterministic generator keeps this property test reproducible while
/// still exercising Unicode text, whitespace, and punctuation-heavy inputs.
fn generated_input(seed: &mut u64) -> String {
    let length = next(seed) as usize % 257;
    let mut input = String::new();
    for _ in 0..length {
        let value = next(seed);
        if !value.is_multiple_of(4) {
            input.push(PUNCTUATION[(value >> 8) as usize % PUNCTUATION.len()]);
            continue;
        }

        let safe = ['a', 'b', 'c', 'd', 'e', 'f', 'g', '中', '文', '🙂'];
        input.push(safe[value as usize % safe.len()]);
    }
    input
}

fn next(seed: &mut u64) -> u64 {
    // Constants from the PCG family; deterministic across platforms.
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed
}

#[test]
fn formatting_is_idempotent_for_generated_inputs() {
    // CI gets a sustained fuzzing run by default. Shorter runs are useful for
    // local iteration: `DJOTLS_FUZZ_DURATION_SECS=5 cargo test --test fmt_fuzz_test`.
    let duration_secs = std::env::var("DJOTLS_FUZZ_DURATION_SECS")
        .ok()
        .map(|value| {
            value.parse::<u64>().unwrap_or_else(|_| {
                panic!("DJOTLS_FUZZ_DURATION_SECS must be an integer: {value:?}")
            })
        })
        .unwrap_or(120);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(duration_secs);
    let mut seed = 0xd707_1d3a_5eed_u64;
    let mut case = 0;
    while case == 0 || std::time::Instant::now() < deadline {
        let input = generated_input(&mut seed);
        for max_cols in [24, 72] {
            let config = djotls::fmt::FmtConfig { max_cols };
            let formatted = djotls::fmt::format(&input, &config);
            let reformatted = djotls::fmt::format(&formatted, &config);
            assert_eq!(
                reformatted, formatted,
                "formatter is not idempotent for generated case {case} at {max_cols} columns; input: {input:?}"
            );
        }
        case += 1;
    }
    eprintln!("formatter fuzz test checked {case} generated inputs in {duration_secs}s");
}
