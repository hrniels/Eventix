# AGENTS.md

This file provides guidance for agentic coding agents operating in this
repository. It summarizes build/test commands, workspace structure, and
project-specific code style and architectural conventions.

The project is a Rust workspace built with Cargo.

------------------------------------------------------------
PROJECT OVERVIEW
------------------------------------------------------------

- Language: Rust (stable toolchain assumed)
- Build system: Cargo (workspace)
- Async runtime: tokio
- CLI parsing: clap (derive API)
- Serialization: serde + serde_json
- Logging: tracing
- Time handling: chrono, chrono-tz

Workspace layout:

- Cargo.toml (workspace root)
- bin/        -> binary crates (app entrypoints)
- libs/       -> reusable domain libraries
- tools/      -> helper scripts (not part of core build)

Workspace members are defined in the root Cargo.toml under [workspace].

------------------------------------------------------------
BUILD COMMANDS
------------------------------------------------------------

Build entire workspace:

    cargo build

Release build:

    cargo build --release

Build a specific crate:

    cargo build -p <crate-name>

Example:

    cargo build -p eventix

Check only (faster, no artifacts):

    cargo check

------------------------------------------------------------
TEST COMMANDS
------------------------------------------------------------

Run all tests in workspace:

    cargo test

Run tests for a specific crate:

    cargo test -p <crate-name>

Example:

    cargo test -p eventix-ical

Run a single test by exact name:

    cargo test <test_name>

Example:

    cargo test recur_parses_weekly

Run tests matching a substring:

    cargo test recur

Run tests in a specific module path:

    cargo test parser::prop

Show output from passing tests:

    cargo test -- --nocapture

Agents should prefer running tests at the smallest relevant scope first
(single crate or single test) before running the full workspace suite.

------------------------------------------------------------
LINTING AND FORMATTING
------------------------------------------------------------

Format code (required before commit):

    cargo fmt

Check formatting in CI mode:

    cargo fmt -- --check

Run Clippy (all targets and features):

    cargo clippy --all-targets --all-features

Treat Clippy warnings as actionable unless there is a strong reason not to.

No custom rustfmt.toml or clippy configuration is present, so default
Rust style conventions apply.

------------------------------------------------------------
ARCHITECTURE GUIDELINES
------------------------------------------------------------

The workspace is layered:

1. libs/ contain domain logic and reusable components.
2. bin/ contain thin binaries that wire together libraries.

Agents should:

- Prefer adding logic to libs/ rather than bin/.
- Keep binaries focused on CLI wiring, configuration, and orchestration.
- Avoid circular dependencies between library crates.
- Preserve clear separation between parsing, domain objects, and state.

Tests are colocated with modules using #[cfg(test)]. Follow this pattern.

------------------------------------------------------------
CODE STYLE GUIDELINES
------------------------------------------------------------

General Rust style:

- Follow standard Rust formatting (rustfmt defaults).
- Use explicit imports; avoid glob imports unless clearly justified.
- Group imports: std, external crates, then internal crates.
- Keep modules small and focused.
- Add comments only when they explain non-obvious decisions; prefer "why" over "what".
- For larger or complex blocks, add a brief high-level comment describing intent or structure.
- When a comment applies to the whole function, prefer a doc comment on the function itself.

Naming conventions:

- Types: PascalCase (e.g., EventState)
- Functions: snake_case (e.g., parse_recur_rule)
- Modules/files: snake_case
- Constants: SCREAMING_SNAKE_CASE
- Traits: PascalCase, descriptive (e.g., CalendarProvider)

Error handling:

- Prefer Result<T, E> over panicking.
- Use thiserror for structured domain errors.
- Use anyhow primarily at binary boundaries.
- Avoid unwrap() and expect() in non-test code unless logically guaranteed.
- Propagate errors with ?.

Async code:

- Use tokio primitives consistently.
- Avoid blocking calls inside async contexts.
- Prefer structured concurrency and explicit task ownership.

Serialization:

- Use serde derive macros.
- Keep serialized representations stable unless versioning is introduced.

Clap usage:

- Use derive API for CLI definitions.
- Keep argument parsing in bin/ crates.
- Delegate execution logic into libs/.

Testing:

- Write focused unit tests next to the code under test.
- Prefer deterministic tests.
- Cover parsing edge cases and recurrence logic thoroughly.
- Use descriptive test names.
- Always place test modules at the very end of the file.
- Name the test module exactly `tests` (i.e., `#[cfg(test)] mod tests`).
- Do not create multiple test modules in the same file.

------------------------------------------------------------
ADDING NEW CODE
------------------------------------------------------------

When adding features:

- Determine whether it belongs in an existing lib crate.
- Only create a new crate if the boundary is clear and reusable.
- Update Cargo.toml dependencies explicitly.
- Avoid adding heavy dependencies without strong justification.
- Code shall never produce warnings.
- Use cargo clippy to ensure that idiomatic Rust code is written.

Public APIs in libs/ should:

- Be minimal and intentional.
- Hide internal implementation details.
- Avoid leaking external crate types in public interfaces unless stable.

------------------------------------------------------------
SCRIPTS AND TOOLS
------------------------------------------------------------

The tools/ directory contains helper scripts (Python and shell).
They are not part of the Cargo build graph.

Agents should not modify tooling unless explicitly requested.

------------------------------------------------------------
CURSOR / COPILOT RULES
------------------------------------------------------------

No .cursor/rules/, .cursorrules, or
.github/copilot-instructions.md files are present.

If such files are added in the future, agents must treat them as
authoritative guidance and update this document accordingly.

------------------------------------------------------------
SAFE WORKFLOW FOR AGENTS
------------------------------------------------------------

Before committing changes:

1. Run cargo fmt
2. Run cargo clippy --all-targets --all-features
3. Run cargo test (or targeted crate tests first)

When modifying parsing or recurrence logic in libs/ical or state logic
in libs/state, always run that crate's full test suite.

Avoid destructive git commands. Do not amend commits unless explicitly
requested. Never reset unrelated changes in the working tree.

------------------------------------------------------------
END
------------------------------------------------------------

This file is intended to help automated agents operate safely and
consistently within this Rust workspace.
