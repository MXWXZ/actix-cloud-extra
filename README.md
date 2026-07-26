# actix-cloud-extra

[![crates.io](https://img.shields.io/crates/v/actix-cloud-extra?label=crates.io&style=flat-square)](https://crates.io/crates/actix-cloud-extra)
[![docs.rs](https://img.shields.io/docsrs/actix-cloud-extra?style=flat-square)](https://docs.rs/actix-cloud-extra/latest)
![license](https://img.shields.io/github/license/mxwxz/actix-cloud-extra?style=flat-square)

Actix Cloud Extra provide extra tools for [Actix Cloud](https://github.com/MXWXZ/actix-cloud).

## Features
Actix Cloud Extra is highly configurable. You can only enable needed features, implement your own feature backend or even use other libraries.

- [hyuuid](#hyuuid) (Default: Enable)
- [api](#api) (Default: Enable)
- [entity](#entity) (Default: Enable)
- [logger](#logger) (Default: Enable)
- [seaorm](#seaorm) (Default: Enable)

## Guide

### hyuuid
UUID v4 support.

### api
Several API helper.

### entity
Seaorm entity helper.

### logger
Provide default logger:
- Keeps only the first target path before `::` (e.g., `project::module1:module2` => `project`). 
- Only keep path starting from the last `src` (e.g., `/a/b/src/c/d/main.rs` => `src/c/d/main.rs`).

```rust
let (logger, _guard) = start_logger(enable, json, verbose, filter);
```

### seaorm
Provide useful macros for [seaorm](https://crates.io/crates/sea-orm).

```rust
#[derive(...)]
#[sea_orm(...)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub created_at: i64,
    pub updated_at: i64,
}

#[entity_id(Uuid::new_v4())]    // generate new for `id` field.
#[entity_timestamp]             // automatically handle `created_at` and `updated_at` field.
impl ActiveModel {}

#[entity_behavior]              // enable `entity_id` and `entity_timestamp`.
impl ActiveModelBehavior for ActiveModel {}
```
