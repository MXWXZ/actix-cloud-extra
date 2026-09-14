#![cfg_attr(docsrs, feature(doc_cfg))]
//! Extra tools for [Actix Cloud](https://github.com/MXWXZ/actix-cloud).
//!
//! Everything is organized in small modules, each behind a cargo feature so
//! you only compile what you need:
//!
//! - `hyuuid`: [`HyUuid`], a strictly hyphenated UUID wrapper used as the universal ID type.
//! - `api`: REST API helpers such as [`api::PageData`], [`api::PaginationParam`] and [`api::Condition`].
//! - `entity`: SeaORM entity helpers like [`entity::VecString`] and [`entity::DefaultColumnTrait`].
//! - `logger`: [`logger::start_logger`], a sensible default logger for Actix Cloud applications.
//! - `seaorm`: SeaORM query helpers like [`api::SelectPage`], [`api::IntoExpr`] and [`api::Condition`].
//! - `macros`: re-export of the proc macros (`default_repo`, `entity_id`, ...).
//! - `utils`: [`utils::StringUtil`] and [`utils::DataUrl`] (also pulled in by `api`).
//!
//! Default features: `api`, `entity`, `logger`, `seaorm` and `macros`, which
//! enable `hyuuid` and `utils` transitively.

#[cfg(feature = "macros")]
pub mod macros {
    pub use actix_cloud_extra_macros::*;
}
#[cfg(feature = "api")]
pub mod api;
#[cfg(feature = "entity")]
pub mod entity;
#[cfg(feature = "hyuuid")]
pub mod hyuuid;
#[cfg(feature = "logger")]
pub mod logger;
#[cfg(feature = "utils")]
pub mod utils;

#[cfg(feature = "hyuuid")]
pub use hyuuid::HyUuid;
