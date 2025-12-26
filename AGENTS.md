# Agent Guidelines for hc-homie5-automation

## Build/Lint/Test Commands
- Build: `cargo build --verbose`
- Test: `cargo test --verbose`
- Single test: `cargo test <test_name>`
- Lint: `cargo clippy -- -D warnings`
- Format: `cargo fmt`

## Code Style Guidelines
- Formatting: rustfmt with 120 char width (see rustfmt.toml)
- Naming: snake_case (vars/fns), CamelCase (types), SCREAMING_SNAKE_CASE (consts)
- Error handling: thiserror for custom errors, color-eyre for reporting
- Imports: Group logically (std, external, local), avoid glob imports
- Documentation: `///` for public API, concise comments
- Types: Strong typing, immutable data, async/await consistency