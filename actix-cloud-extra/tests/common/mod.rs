#![allow(dead_code)]

use actix_cloud_extra::macros::default_repo;
use sea_orm::entity::prelude::*;

pub mod users {
    use actix_cloud_extra::HyUuid;
    use actix_cloud_extra::macros::{entity_behavior, entity_id, entity_timestamp};
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: HyUuid,
        pub name: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    #[entity_id(HyUuid::new())]
    #[entity_timestamp]
    impl ActiveModel {}

    #[entity_behavior]
    impl ActiveModelBehavior for ActiveModel {}
}

pub struct UserRepo;

#[default_repo(users)]
impl UserRepo {
    /// A user-defined method must survive the macro pass.
    pub fn marker() -> &'static str {
        "kept"
    }
}

pub fn ts(s: &str) -> DateTimeUtc {
    s.parse().unwrap()
}

pub fn sample(name: &str) -> users::Model {
    users::Model {
        id: actix_cloud_extra::HyUuid::new(),
        name: name.to_owned(),
        created_at: ts("2024-01-01T00:00:00Z"),
        updated_at: ts("2024-01-01T00:00:00Z"),
    }
}

pub fn count_row(n: i64) -> std::collections::BTreeMap<String, sea_orm::Value> {
    std::collections::BTreeMap::from([("num_items".to_owned(), sea_orm::Value::BigInt(Some(n)))])
}
