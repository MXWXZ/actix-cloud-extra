# actix-cloud-extra

[![crates.io](https://img.shields.io/crates/v/actix-cloud-extra?label=crates.io&style=flat-square)](https://crates.io/crates/actix-cloud-extra)
[![docs.rs](https://img.shields.io/docsrs/actix-cloud-extra?style=flat-square)](https://docs.rs/actix-cloud-extra/latest)
![license](https://img.shields.io/github/license/mxwxz/actix-cloud-extra?style=flat-square)

Actix Cloud Extra provides extra tools for [Actix Cloud](https://github.com/MXWXZ/actix-cloud).

Actix Cloud Extra is highly configurable: you can enable only the features you need, implement your own feature backend or even use other libraries.

| Feature           | Default  | Description                                                     |
| ----------------- | -------- | --------------------------------------------------------------- |
| [hyuuid](#hyuuid) | enabled  | Strictly hyphenated UUID v4 ID type                             |
| [api](#api)       | enabled  | Pagination, query condition builder and common request payloads |
| [entity](#entity) | enabled  | SeaORM entity helpers                                           |
| [logger](#logger) | enabled  | Default logger for Actix Cloud applications                     |
| [seaorm](#seaorm) | enabled  | SeaORM query helpers                                            |
| [macros](#macros) | enabled  | Proc macros (`default_viewer`, `entity_id`, ...)                |
| [utils](#utils)   | disabled | String and data URL helpers (pulled in by `api`)                |

## Installation

```toml
[dependencies]
actix-cloud-extra = "0.1"
```

Or with only the features you need:

```toml
[dependencies]
actix-cloud-extra = { version = "0.1", default-features = false, features = ["hyuuid", "utils"] }
```

Requires a recent stable Rust toolchain (edition 2024).

## Guide

### hyuuid

`HyUuid` is the universal ID type: a wrapped `Uuid` strictly serialized/deserialized as a 36-char hyphenated string.

```rust
use actix_cloud_extra::HyUuid;

let id = HyUuid::new(); // random UUID v4
let id = HyUuid::parse("550e8400-e29b-41d4-a716-446655440000")?; // strictly UUID v4
assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
```

JSON serialization only accepts the 36-char hyphenated form; other textual formats (simple, braced, URN) and other UUID versions are rejected.

### api

API helpers for pagination and dynamic queries.

`PaginationParam` defaults to page 1, size 10 and validates `page >= 1`, `1 <= size <= 100`:

```json
{ "page": 2, "size": 20 }
```

`Condition` is the query hub: build filters, sort and pagination fluently, then run it.

```rust
use actix_cloud_extra::api::{Condition, IntoExpr, PageData, PaginationParam};

let page: PaginationParam = ...;    // from request
let sort: Option<String> = ...;     // e.g. "-created_at,username"
let name = &"ad".to_string();

let cond = Condition::new_all()
    .add(name.like_expr(user::Column::Username))    // username LIKE '%ad%'
    .parse_sort_option(&sort, vec![
        user::Column::Id.into(),
        user::Column::CreatedAt.into(),
    ])
    .add_page(page);

let (data, total) = cond.select_page(user::Entity::find(), &db).await?;
let rsp = PageData::new((data, total));
```

Sort names not present in the whitelist are silently ignored, so `parse_sort_option` is safe to feed straight from user input. Use `Condition::add_time` with `TimeParam` (deserialized from datetime strings like `2024-01-01T00:00:00`, `T` separator required) to filter on creation/update time ranges, see [entity](#entity). `IDsReq` / `OptionIDsReq` carry a list of IDs and reject duplicates via validation.

### entity

SeaORM entity helpers.

- `VecString`: `Vec<String>` stored as a JSON array column.
- `DefaultColumnTrait`: exposes the `created_at`/`updated_at` columns; implemented automatically by the `entity_timestamp` macro and consumed by `Condition::add_time`.

```rust
use actix_cloud_extra::entity::VecString;

#[derive(...)]
pub struct Model {
    pub tags: VecString,    // stored as a JSON array
}
```

### logger

Provide default logger:
- Keeps only the first target path before `::` (e.g., `project::module1::module2` => `project`). 
- Keeps only the path starting from the last `src` (e.g., `/a/b/src/c/d/main.rs` => `src/c/d/main.rs`).

```rust
use actix_cloud_extra::logger::start_logger;

let (logger, _guard) = start_logger(
    /* enable  */ true,
    /* json    */ false,
    /* verbose */ true,        // file name, line number and DEBUG level
    /* filter  */ |_| true,
);
```

Returns `(None, None)` when `enable` is `false`; the guard stops logging when dropped.

### seaorm

Typical entity setup with the provided macros:

```rust
use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, Default, DeriveEntityModel)]
#[sea_orm(table_name = "user")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: HyUuid,
    pub username: String,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[entity_id(HyUuid::new())]     // generate `id` on insert when not set
#[entity_timestamp]             // handle `created_at` and `updated_at`
impl ActiveModel {}

#[entity_behavior]              // enable `entity_id` and `entity_timestamp` on save
impl ActiveModelBehavior for ActiveModel {}

pub struct UserViewer;

#[default_viewer(user)]         // find / find_by_id / delete_all / delete / count
impl UserViewer {}

#[partial_entity(user::Model)]
#[derive(serde::Serialize)]
pub struct UserRsp {
    pub id: HyUuid,
    pub username: String,
}

let rsp: UserRsp = model.into();
```

### macros

Re-export of the proc macros (`default_viewer`, `entity_id`, `entity_timestamp`, `entity_behavior`, `partial_entity`), see [seaorm](#seaorm) for usage.

### utils

String helpers (`StringUtil`, implemented for all `AsRef<str>`) and data URL helpers (`DataUrl`).

```rust
use actix_cloud_extra::utils::{DataUrl, StringUtil};

assert!("0123456789abcdef".is_hex());
assert_eq!("a%b_c\\d".like_escape(), "a\\%b\\_c\\\\d");

let url = DataUrl::new(bytes).unwrap().encode();    // data:image/png;base64,...
let data = DataUrl::decode(&url).unwrap();
```

## License

MIT, see [LICENSE](LICENSE).
