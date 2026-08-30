# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Rust workspace providing extra tools (helpers, proc macros) for [Actix Cloud](https://github.com/MXWXZ/actix-cloud). Edition 2024. Two crates:

- `actix-cloud-extra/` — the library. Every module is behind a cargo feature: `hyuuid`, `api`, `entity`, `logger`, `utils`, `seaorm`, `macros`. Default = all except `utils` standalone (it's pulled in via `api`).
- `actix-cloud-extra-macros/` — proc macros (`default_viewer`, `entity_id`, `entity_timestamp`, `entity_behavior`, `partial_entity`). Has a `seaorm` feature toggled by the main crate's `seaorm` feature.

## Commands

- `make check` — clippy (`--all-features -D clippy::all`) + `cargo fmt --check`. CI gate.
- `make test` — `cargo test --all-features`
- Single test: `cargo test --all-features hyuuid::tests::parse_ok` (tests are inline `#[cfg(test)]` modules in `hyuuid.rs` and `utils.rs`)
- `make doc` — build docs on **nightly** with `--cfg docsrs` (matches docs.rs config)

Because everything is feature-gated, `--all-features` builds can pass while a single feature fails to build alone. When changing feature wiring or cfg gates, verify with e.g. `cargo check --no-default-features` and per-feature `cargo check -F <feature>`.

## Architecture

### Macros generate code that references the main crate's paths

`actix-cloud-extra-macros` has no dependency on the main crate, but its expanded code hard-codes paths like `actix_cloud_extra::api::Condition`, `actix_cloud_extra::HyUuid`, and `actix_cloud_extra::entity::DefaultColumnTrait` (see `default_viewer` and `entity_timestamp` in `actix-cloud-extra-macros/src/lib.rs`). Renaming/moving public items in the main crate silently breaks the macros at downstream expansion time — keep the two crates in sync conceptually.

### Entity macro stacking (seaorm)

`entity_id` and `entity_timestamp` work by appending private helper methods (`entity_id`, `entity_timestamp`) to the same `impl ActiveModel` block. `entity_behavior` then appends `before_save` to that block which calls both helpers — so all three must be applied to the same impl, and `#[entity_behavior]` goes on `impl ActiveModelBehavior`, not `impl ActiveModel`. `entity_timestamp` additionally emits `impl DefaultColumnTrait for Column`, which is what `api::Condition::add_time` consumes.

### `Condition` is the query hub

`api::Condition` wraps a `sea_orm::Condition` + optional `PaginationParam` + sort list. `default_viewer`-generated CRUD (`find`, `count`, …) all funnel through `Condition::build`/`select_page`. `PaginationParam` applies `#[validate]` ranges (`page >= 1`, `1 <= size <= 100`) and is used both for SQL pagination (`SelectPage` trait) and in-memory splitting (`split`).

### `HyUuid` is the universal ID type

Wrapped `uuid::Uuid` serialized/deserialized strictly as a 36-char hyphenated string, `DeriveValueType` for SeaORM. `TryFromU64` is deliberately unimplemented — entities using it must have non-auto-increment PKs. `entity_id(Uuid::new_v4())` is the typical id generator.

## Release

CI (`.github/workflows/release.yml`) triggers on tags `v*`: runs `make check` + `make test`, then publishes **the macros crate first**, then the main crate, and creates a draft GitHub release. Bump versions manually in both `Cargo.toml`s before tagging (commit style: `v0.1.3`).
