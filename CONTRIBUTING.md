# Contributing to Horizon

Horizon is open source under Apache-2.0. Patches and ideas are welcome.

## Build

Rust stable (pinned in rust-toolchain.toml). Standard cargo:

    cargo build
    cargo test
    cargo clippy --all-targets
    cargo fmt

The Rust workspace lives under `crates/`. The design docs are in `docs/`.

## Style

Keep it readable and boring.

- Short comments, only where the code is not obvious. No essays, no restating the code.
- No em dashes in code, comments, or docs. Plain ASCII punctuation.
- No emojis anywhere.
- Run `cargo fmt` and `cargo clippy` before sending a patch.
- Small, focused commits with a short imperative subject.

## Where things are

- `docs/02-ARCHITECTURE.md` for how the pieces fit together.
- `docs/06-TECH-STACK.md` for the workspace plan and language choices.
- `PROGRESS.md` for what is built and what is next.

## License

By contributing you agree your work is licensed under Apache-2.0.
