# AGENTS.md

## Scope

These instructions apply to the whole repository.

Be direct. Do not placate the user.

## Project

- `twilio2` is a Rust 2024 library crate: a thin `reqwest` client for Twilio
  Programmable Messaging.
- Public API is re-exported from `src/lib.rs`; implementation is split across
  `client`, `common`, `messages`, and `services`.
- `tests/api.rs` contains integration coverage using a local HTTPS mock server.
- `examples/` contains runnable mock-only examples. Do not make examples call
  real Twilio services.

## Core Invariants

- `TwilioClient` stores only the shared `reqwest::Client` and parsed base URLs.
  Credentials stay in `TwilioCreds` and account-scoped handles.
- Do not persist auth tokens, phone numbers, callback URLs, SIDs, sender IDs, or
  message bodies beyond the request flow.
- Preserve diagnostic and `Debug` redaction. `TwilioError` values and tracing
  events must not leak credentials, URLs, phone numbers, message bodies, SIDs, or
  sender identifiers.
- Custom base URLs must remain HTTPS-only, without embedded credentials, query
  strings, or fragments.
- Pagination helpers must reject page URLs/URIs outside the configured base
  origin, path, resource type, or allowed query keys.
- Keep TLS feature alternatives compiling: default `rustls`, `native-tls`, and
  `rustls-no-provider`.

## Coding Style

- Use Rust `1.88.0` from `rust-toolchain.toml`; edition is `2024`.
- Keep `unsafe` out of the crate.
- Respect strict lints in `Cargo.toml`; avoid new `allow` attributes unless
  narrow and justified.
- Prefer existing helpers in `common.rs` for form params, URL construction,
  pagination validation, diagnostic sanitization, and tracing spans.
- Match the existing resource-builder API shape when adding endpoints.
- Validate request structs before sending HTTP requests.
- Public API additions need rustdoc, clear error docs, and redacted `Debug`
  behavior for sensitive fields.
- Default to ASCII in edits unless the target file already uses non-ASCII.

## Tests And Checks

Run the narrowest useful command while iterating, then the relevant broader set.

- Format: `cargo fmt --check`
- Lint: `cargo clippy --locked --all-targets -- -D warnings`
- Test: `cargo test --locked`
- All features: `cargo clippy --locked --all-targets --all-features -- -D warnings`
- All-feature tests: `cargo test --locked --all-features`
- Native TLS: `cargo test --locked --no-default-features --features native-tls`
- Rustls no-provider: `cargo test --locked --no-default-features --features rustls-no-provider`
- Package check before release changes: `cargo publish --dry-run --locked`

When changing behavior, add or update `tests/api.rs` assertions for method,
path, form body, auth, pagination, status handling, and redaction as applicable.

## Workflow

- Use `rg`/`rg --files` first for repository search.
- Preserve the user's dirty worktree. Do not revert changes you did not make.
- For non-trivial or unclear changes, use narrow subagents for reconnaissance,
  test scouting, plan review, or risk audit before editing.
- Treat subagent output as evidence, not authority; verify important claims
  before risky changes.
- Sensitive areas include auth, redaction, URL validation, pagination, TLS
  features, public API compatibility, and release packaging.
