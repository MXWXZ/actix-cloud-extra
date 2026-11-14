# actix-cloud-extra

[![crates.io](https://img.shields.io/crates/v/actix-cloud-extra?label=crates.io&style=flat-square)](https://crates.io/crates/actix-cloud-extra)
[![docs.rs](https://img.shields.io/docsrs/actix-cloud-extra?style=flat-square)](https://docs.rs/actix-cloud-extra/latest)
[![CI](https://github.com/MXWXZ/actix-cloud-extra/actions/workflows/ci.yml/badge.svg)](https://github.com/MXWXZ/actix-cloud-extra/actions/workflows/ci.yml)
![license](https://img.shields.io/github/license/mxwxz/actix-cloud-extra?style=flat-square)

[English](README.md) | 简体中文

为基于 SeaORM 的 [Actix Cloud](https://github.com/MXWXZ/actix-cloud) 服务提供的扩展工具：严格的 UUID ID 类型、分页与查询辅助、实体/过程宏，以及默认日志器。

所有模块都通过 cargo feature 控制，可以只编译需要的部分。

## 功能特性

| Feature | 默认 | 启用的依赖 | 说明 |
| --- | --- | --- | --- |
| `hyuuid` | 是 | `uuid`、`thiserror`、`serde` | `HyUuid`：严格使用连字符格式的 UUID v4 ID 类型，以及 crate 级 `Error` / `Result` |
| `api` | 是 | `hyuuid`、`utils`、`validator` | 分页、`Condition` 与常用请求载荷 |
| `entity` | 是 | `seaorm`、`serde`、`serde_json` | `VecString` 与 `DefaultColumnTrait` |
| `logger` | 是 | `actix-cloud/logger` | `start_logger` 默认日志器 |
| `seaorm` | 是 | `sea-orm` | `Condition`、`SelectPage`、`IntoExpr` 辅助 |
| `macros` | 是 | 过程宏 crate | `default_repo`、`entity_id`、`entity_timestamp` 等 |
| `utils` | 否 | `base64`、`infer` | `StringUtil` 与 `DataUrl` |

默认启用 `api`、`entity`、`logger`、`seaorm`、`macros`；其中 `api` 会带入 `hyuuid` 和 `utils`，`entity` 会带入 `seaorm`。

## 安装

```toml
[dependencies]
actix-cloud-extra = "0.1"
```

或只启用需要的 feature：

```toml
[dependencies]
actix-cloud-extra = { version = "0.1", default-features = false, features = ["hyuuid", "utils"] }
```

**MSRV：** Rust 1.94（edition 2024）。

## 使用指南

### hyuuid

`HyUuid` 是通用 ID 类型：包装 `Uuid`，序列化和反序列化时严格使用 36 字符连字符格式。

```rust
use actix_cloud_extra::HyUuid;

let id = HyUuid::new(); // 随机 UUID v4
let id = HyUuid::parse("550e8400-e29b-41d4-a716-446655440000")?;
assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
```

只接受 36 字符连字符格式；simple、braced 和 URN 形式会被拒绝，非 nil 值必须是 version 4。nil UUID 会被接受，以便 `Default` / `nil()` 通过 serde 往返。启用 `seaorm` 后，`HyUuid` 还会派生 `DeriveValueType`，可直接作为 SeaORM 列类型使用。

### api

分页与动态查询辅助（需要 `api` feature；`seaorm` 会提供查询构建器）。

`PaginationParam` 默认 page 1、size 10，并校验 `page >= 1`、`1 <= size <= 100`：

```json
{ "page": 2, "size": 20 }
```

校验是 opt-in 的：对来自请求的参数调用 `validate()`。`select_page` 还会拒绝 `size == 0` 和会溢出 `u64` 的偏移量，而不是 panic。调用 `validate()` 需要添加匹配的 `validator = "0.20"` 依赖（`actix-web-validator` 7 使用同一版本）。

`Condition` 是查询中枢：以链式方式构建过滤、排序和分页，然后执行。

```rust
use actix_cloud_extra::api::{Condition, IntoExpr, PageData, PaginationParam};
use sea_orm::DatabaseConnection;

let page = PaginationParam { page: 1, size: 20 }; // 校验来自请求的参数
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

不在白名单中的排序字段会被静默忽略，因此 `parse_sort_option` 可以直接接收用户输入。`Condition::add_time` 接收由 RFC 3339 字符串反序列化而来的 `TimeParam`（必须带时区偏移）：

```json
{ "created_start": "2024-01-01T00:00:00Z", "updated_end": "2024-01-31T23:59:59Z" }
```

`IDsReq` / `OptionIDsReq` 携带 ID 列表，并通过 `validate()` 拒绝重复项。请在请求层限制列表长度以及排序/搜索输入的长度。

### 错误处理

所有可失败 API 都返回 `actix_cloud_extra::Result<T>`，即 `Result<T, actix_cloud_extra::Error>` 的别名。`Error` 是非穷尽的 `thiserror` 枚举：包裹 `uuid::Error`，在启用 `seaorm` 时还会包裹 `sea_orm::DbErr` 以及分页错误。它实现了 `std::error::Error + Send + Sync`，因此兼容 actix-cloud 的 `ResponseError`，在仍然返回 `anyhow::Result` 的 handler 中也可以直接 `?` / `.context()`。

```rust
use actix_cloud_extra::Error;

fn classify(err: Error) {
    match err {
        Error::UuidLength | Error::UuidVersion | Error::Uuid(_) => { /* ID 非法 */ }
        Error::Db(_) => { /* 数据库错误 */ }
        _ => { /* 分页错误及未来新增变体 */ }
    }
}
```

### entity

SeaORM 实体辅助：

- `VecString`：以 JSON 数组列存储的 `Vec<String>`。
- `DefaultColumnTrait`：暴露 `created_at` / `updated_at` 列；由 `entity_timestamp` 宏自动实现，并被 `Condition::add_time` 使用。

```rust
use actix_cloud_extra::entity::VecString;

pub struct Model {
    // ...
    pub tags: VecString, // 以 JSON 数组列存储
}
```

### logger

Actix Cloud 应用的默认日志器：

- target 只保留 `::` 前的第一段（`project::module1::module2` => `project`）。
- 文件名只保留最后一个 `src` 开始的部分（`/a/b/src/c/d/main.rs` => `src/c/d/main.rs`）。

```rust
use actix_cloud_extra::logger::start_logger;

let (logger, _guard) = start_logger(
    /* enable  */ true,
    /* json    */ false,
    /* verbose */ true,        // 文件名、行号与 DEBUG 级别
    /* filter  */ |_| true,    // fn(&LogItem) -> bool
);
```

`enable` 为 `false` 时返回 `(None, None)`；guard 被丢弃时停止日志。`start_logger` 只能在一个进程启动时调用一次：它会安装全局 tracing subscriber，如果已存在则 panic。transformer 不会转义 target/filename 中的控制字符，请勿把不可信数据放进这些字段。

### seaorm

使用提供的宏的典型实体配置：

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

#[entity_id(HyUuid::new())]     // 插入时生成 id（仅当未设置时）
#[entity_timestamp]             // 插入时设置 created_at/updated_at，更新时只刷新 updated_at
impl ActiveModel {}

#[entity_behavior]              // 在 before_save 中执行上述两个辅助方法
impl ActiveModelBehavior for ActiveModel {}

pub struct UserRepo;

#[default_repo(user)]           // find / find_by_id / delete_all / delete / count
impl UserRepo {}

#[partial_entity(user::Model)]  // 实现 From<user::Model>
#[derive(Debug, serde::Serialize)]
pub struct UserRsp {
    pub id: HyUuid,
    pub username: String,
}
```

宏展开后的代码会引用 `sea_orm`、`chrono`、`async-trait` 和 `actix_cloud_extra::Result`（crate 自身的错误类型），因此 `sea_orm`、`chrono` 和 `async-trait` 必须是你的 crate 的直接依赖；不再需要 `anyhow` / `thiserror`（同时需要在使用宏的模块中 `use sea_orm::entity::prelude::*;`，以提供 `DateTimeUtc`、`EntityTrait` 和 `PaginatorTrait`）：

```toml
[dependencies]
actix-cloud-extra = { version = "0.1", features = ["entity", "macros", "seaorm"] }
async-trait = "0.1"
chrono = "0.4"
sea-orm = "2"
serde = { version = "1", features = ["derive"] }
```

请按应用需要添加 SeaORM 的数据库驱动/运行时 feature。生成的 repo 方法不做授权或租户隔离；请为 `find_by_id` / `delete` 增加所有权检查，不要直接暴露 `delete_all`。不要把不可信的请求数据映射到 `id` 或时间戳字段。

### macros

过程宏的重新导出（`default_repo`、`entity_id`、`entity_timestamp`、`entity_behavior`、`partial_entity`），用法见 [seaorm](#seaorm)。

### utils

字符串辅助（`StringUtil`，为所有 `AsRef<str>` 实现）与 data URL 辅助（`DataUrl`）。

```rust
use actix_cloud_extra::utils::{DataUrl, StringUtil};

assert!("0123456789abcdef".is_hex());
assert!(!"".is_hex());
assert_eq!("a%b_c\\d".like_escape(), "a\\%b\\_c\\\\d");

let url = DataUrl::new(bytes).unwrap().encode();    // data:image/png;base64,...
let data = DataUrl::decode(&url).unwrap();
```

`DataUrl::decode` 会忽略声明的前缀，根据字节内容推断 MIME 类型；请把它当作不可信提示，并在请求层限制输入大小。

## 安全注意事项

- 校验来自请求的 `PaginationParam`；`select_page` 会拒绝 `size == 0` 和溢出偏移，但不会替你强制 size 上限。
- 未调用 `add_page` 的 `Condition` 会加载整个结果集。面向请求的查询应始终分页，并设置上限。
- 在边缘层限制 ID 列表、排序字符串、LIKE 搜索词和 data URL 的长度。
- `DataUrl::decode` 根据内容推断 MIME 并忽略声明的 MIME。对外提供解码数据前应使用 MIME allowlist。
- `HyUuid` 接受 nil 以及 version nibble 为 4 的非 RFC variant；不要把 nil 当作已授权的 ID。
- `start_logger` 在进程启动时只能调用一次；如果已存在全局 subscriber 会 panic。
- 生成的 repo 方法不做授权隔离；不要暴露 `delete_all`，也不要把不可信输入映射到 `id` / `created_at`。

## License

MIT，见 [LICENSE](LICENSE)。
