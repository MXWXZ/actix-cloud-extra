# actix-cloud-extra

[![crates.io](https://img.shields.io/crates/v/actix-cloud-extra?label=crates.io&style=flat-square)](https://crates.io/crates/actix-cloud-extra)
[![docs.rs](https://img.shields.io/docsrs/actix-cloud-extra?style=flat-square)](https://docs.rs/actix-cloud-extra/latest)
[![CI](https://github.com/MXWXZ/actix-cloud-extra/actions/workflows/ci.yml/badge.svg)](https://github.com/MXWXZ/actix-cloud-extra/actions/workflows/ci.yml)
![license](https://img.shields.io/github/license/mxwxz/actix-cloud-extra?style=flat-square)

English | [简体中文](https://github.com/MXWXZ/actix-cloud-extra/blob/main/README_CN.md)

Extra tools for building [Actix Cloud](https://github.com/MXWXZ/actix-cloud) services on top of SeaORM: a strict UUID ID type, pagination and query helpers, entity/proc macros, and a default logger.

Every module is behind a cargo feature, so you can compile only what you use.

## Features

| Feature | Default | Enables | Description |
| --- | --- | --- | --- |
| `hyuuid` | yes | `uuid`, `thiserror`, `serde` | `HyUuid`, a strictly hyphenated UUID v4 ID type, plus the crate-wide `Error` / `Result` |
| `api` | yes | `hyuuid`, `utils`, `validator` | Pagination, `Condition`, and common request payloads |
| `entity` | yes | `seaorm`, `serde`, `serde_json` | `VecString` and `DefaultColumnTrait` |
| `logger` | yes | `actix-cloud/logger` | `start_logger`, a default logger |
| `seaorm` | yes | `sea-orm` | `Condition`, `SelectPage`, and `IntoExpr` helpers |
| `macros` | yes | proc-macro crate | `default_repo`, `entity_id`, `entity_timestamp`, ... |
| `utils` | no | `base64`, `infer` | `StringUtil` and `DataUrl` |

The default features are `api`, `entity`, `logger`, `seaorm`, and `macros`; `api` pulls in `hyuuid` and `utils`, and `entity` pulls in `seaorm`.

## Installation

```toml
[dependencies]
actix-cloud-extra = "0.2"
```

Or with only the features you need:

```toml
[dependencies]
actix-cloud-extra = { version = "0.2", default-features = false, features = ["hyuuid", "utils"] }
```

**MSRV:** Rust 1.94 (edition 2024).

## Guide

### hyuuid

`HyUuid` is the universal ID type: a wrapped `Uuid` serialized and deserialized as a 36-char hyphenated string.

```rust
use actix_cloud_extra::HyUuid;

let id = HyUuid::new(); // random UUID v4
let id = HyUuid::parse("550e8400-e29b-41d4-a716-446655440000")?;
assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
```

Only the 36-char hyphenated form is accepted; simple, braced, and URN forms are rejected, and non-nil values must be version 4. The nil UUID is accepted so `Default` / `nil()` can roundtrip through serde. With the `seaorm` feature, `HyUuid` also derives `DeriveValueType` and can be used directly as a SeaORM column type.

### api

Pagination and dynamic query helpers (requires the `api` feature; `seaorm` adds the query builder).

`PaginationParam` defaults to page 1, size 10 and validates `page >= 1`, `1 <= size <= 100`:

```json
{ "page": 2, "size": 20 }
```

Validation is opt-in: call `validate()` on request-derived values. `select_page` additionally rejects `size == 0` and offsets that would overflow `u64` instead of panicking. To call `validate()`, add the matching `validator = "0.20"` dependency (`actix-web-validator` 7 uses the same version).

`Condition` is the query hub: build filters, sorting, and pagination fluently, then execute it.

```rust
use actix_cloud_extra::api::{Condition, IntoExpr, PageData, PaginationParam};
use sea_orm::DatabaseConnection;

let page = PaginationParam { page: 1, size: 20 }; // validate request-derived values
let sort = Some("-created_at,username".to_owned());
let name = "ad".to_string();

let cond = Condition::new_all()
    .add((&name).like_expr(user::Column::Username)) // username LIKE '%ad%'
    .parse_sort_option(&sort, vec![
        user::Column::Id.into(),
        user::Column::CreatedAt.into(),
    ])
    .add_page(page);

let db: &DatabaseConnection = ...;
let (data, total) = cond.select_page(user::Entity::find(), &db).await?;
let rsp = PageData::new((data, total));
```

Sort names not present in the whitelist are silently ignored, so `parse_sort_option` is safe to feed straight from user input. `Condition::add_time` consumes a `TimeParam` deserialized from RFC 3339 strings (a timezone offset is required):

```json
{ "created_start": "2024-01-01T00:00:00Z", "updated_end": "2024-01-31T23:59:59Z" }
```

`IDsReq` / `OptionIDsReq` carry a list of IDs and reject duplicates via `validate()`. Cap the list length and the sort/search input at the request layer.

### Errors

All fallible APIs return `actix_cloud_extra::Result<T>`, an alias for `Result<T, actix_cloud_extra::Error>`. `Error` is a non-exhaustive `thiserror` enum: it wraps `uuid::Error`, and with `seaorm` also `sea_orm::DbErr` plus the pagination errors. It implements `std::error::Error + Send + Sync`, so it works with actix-cloud's `ResponseError` and with `?` / `.context()` in handlers that still return `anyhow::Result`.

```rust
use actix_cloud_extra::Error;

fn classify(err: Error) {
    match err {
        Error::UuidLength | Error::UuidVersion | Error::Uuid(_) => { /* invalid ID */ }
        Error::Db(_) => { /* database failure */ }
        _ => { /* pagination errors and future variants */ }
    }
}
```

### entity

SeaORM entity helpers:

- `VecString`: a `Vec<String>` stored as a JSON array column.
- `DefaultColumnTrait`: exposes the `created_at` / `updated_at` columns; implemented by the `entity_timestamp` macro and consumed by `Condition::add_time`.

```rust
use actix_cloud_extra::entity::VecString;

pub struct Model {
    // ...
    pub tags: VecString, // stored as a JSON array column
}
```

### logger

A default logger for Actix Cloud applications:

- Keeps only the first target path before `::` (`project::module1::module2` => `project`).
- Keeps only the path starting from the last `src` (`/a/b/src/c/d/main.rs` => `src/c/d/main.rs`).

```rust
use actix_cloud_extra::logger::start_logger;

let (logger, _guard) = start_logger(
    /* enable  */ true,
    /* json    */ false,
    /* verbose */ true,        // file name, line number and DEBUG level
    /* filter  */ |_| true,    // fn(&LogItem) -> bool
);
```

Returns `(None, None)` when `enable` is `false`; the guard stops logging when dropped. Call `start_logger` once at startup: it installs a global tracing subscriber and panics if one is already installed. The transformer does not escape control characters in target/filename, so keep untrusted data out of those fields.

### seaorm

Typical entity setup with the provided macros:

```rust
use actix_cloud_extra::HyUuid;
use actix_cloud_extra::macros::{
    default_repo, entity_behavior, entity_id, entity_timestamp, partial_entity,
};
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: HyUuid,
    pub username: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

#[entity_id(HyUuid::new())]     // generate `id` on insert when not set
#[entity_timestamp]             // created_at/updated_at on insert, updated_at on update
impl ActiveModel {}

#[entity_behavior]              // run both helpers in before_save
impl ActiveModelBehavior for ActiveModel {}

pub struct UserRepo;

#[default_repo(user)]           // find / find_by_id / delete_all / delete / count
impl UserRepo {}

#[partial_entity(user::Model)]  // implement From<user::Model>
#[derive(Debug, serde::Serialize)]
pub struct UserRsp {
    pub id: HyUuid,
    pub username: String,
}
```

The macros expand to code that references `sea_orm`, `chrono`, `async-trait`, and `actix_cloud_extra::Result` (the crate's own error type), so `sea_orm`, `chrono`, and `async-trait` must be direct dependencies of your crate; `anyhow` / `thiserror` are not needed (and `sea_orm::entity::prelude::*` must be in scope for `DateTimeUtc`, `EntityTrait`, and `PaginatorTrait`):

```toml
[dependencies]
actix-cloud-extra = { version = "0.2", features = ["entity", "macros", "seaorm"] }
async-trait = "0.1"
chrono = "0.4"
sea-orm = "2"
serde = { version = "1", features = ["derive"] }
```

Add the SeaORM driver/runtime features your application needs. The generated repo methods perform no authorization or tenant scoping; wrap `find_by_id` / `delete` with ownership checks and do not expose `delete_all` directly. Do not map untrusted request data into `id` or timestamp fields.

### macros

Re-export of the proc macros (`default_repo`, `entity_id`, `entity_timestamp`, `entity_behavior`, `partial_entity`); see [seaorm](#seaorm) for usage.

### utils

String helpers (`StringUtil`, implemented for all `AsRef<str>`) and data URL helpers (`DataUrl`).

```rust
use actix_cloud_extra::utils::{DataUrl, StringUtil};

assert!("0123456789abcdef".is_hex());
assert!(!"".is_hex());
assert_eq!("a%b_c\\d".like_escape(), "a\\%b\\_c\\\\d");

let url = DataUrl::new(bytes).unwrap().encode();    // data:image/png;base64,...
let data = DataUrl::decode(&url).unwrap();
```

`DataUrl::decode` ignores the declared prefix and infers the MIME type from the bytes; treat it as an untrusted hint and bound the input size at the request layer.

## Security notes

- Validate request-derived `PaginationParam`; `select_page` rejects `size == 0` and overflowing offsets, but does not enforce the size limit for you.
- A `Condition` without `add_page` loads the whole result set. Always paginate request-facing queries and enforce an upper bound.
- Cap ID-list, sort-string, LIKE-term, and data-URL lengths at the edge.
- `DataUrl::decode` infers MIME from content and ignores the declared MIME. Apply a MIME allowlist before serving decoded data.
- `HyUuid` accepts nil and non-RFC variants as long as the version nibble is 4; do not treat nil as an authorized ID.
- Call `start_logger` exactly once at startup; it panics if a global subscriber already exists.
- Generated repo methods do no authorization scoping; do not expose `delete_all` and do not map untrusted input into `id` / `created_at`.

## License

MIT, see [LICENSE](https://github.com/MXWXZ/actix-cloud-extra/blob/main/LICENSE).
