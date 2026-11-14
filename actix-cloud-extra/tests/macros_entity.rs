#![cfg(all(
    feature = "api",
    feature = "entity",
    feature = "hyuuid",
    feature = "macros",
    feature = "seaorm"
))]

mod common;

use actix_cloud_extra::HyUuid;
use actix_cloud_extra::api::{Condition, TimeParam};
use actix_cloud_extra::macros::partial_entity;
use common::{sample, users};
use futures::executor::block_on;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, DbBackend, MockDatabase, QueryTrait};

fn mock_connection() -> sea_orm::DatabaseConnection {
    // `before_save` ignores the connection, so an empty mock is enough.
    MockDatabase::new(DbBackend::Sqlite).into_connection()
}

fn active(name: &str) -> users::ActiveModel {
    users::ActiveModel {
        id: ActiveValue::NotSet,
        name: ActiveValue::Set(name.to_owned()),
        created_at: ActiveValue::NotSet,
        updated_at: ActiveValue::NotSet,
    }
}

// ---- normal path ----

#[test]
fn normal_before_save_insert_sets_id_and_timestamps() {
    let db = mock_connection();
    let before = chrono::Utc::now();
    let saved = block_on(active("alice").before_save(&db, true)).unwrap();
    let after = chrono::Utc::now();

    let ActiveValue::Set(id) = saved.id else {
        panic!("id should be generated on insert");
    };
    assert!(!id.is_nil());

    let ActiveValue::Set(created) = saved.created_at else {
        panic!("created_at should be set on insert");
    };
    let ActiveValue::Set(updated) = saved.updated_at else {
        panic!("updated_at should be set on insert");
    };
    assert_eq!(created, updated, "both timestamps come from one Utc::now()");
    assert!(created >= before && created <= after);
}

#[test]
fn normal_before_save_update_only_updated_at() {
    let db = mock_connection();
    let old = chrono::Utc::now() - chrono::Duration::hours(1);
    let id = HyUuid::new();
    let am = users::ActiveModel {
        id: ActiveValue::Set(id),
        name: ActiveValue::Set("bob".to_owned()),
        created_at: ActiveValue::Set(old),
        updated_at: ActiveValue::Set(old),
    };

    let saved = block_on(am.before_save(&db, false)).unwrap();
    assert!(matches!(saved.id, ActiveValue::Set(v) if v == id));
    assert!(matches!(saved.created_at, ActiveValue::Set(v) if v == old));

    let ActiveValue::Set(updated) = saved.updated_at else {
        panic!("updated_at should be refreshed on update");
    };
    assert!(updated > old);
}

#[test]
fn normal_default_column_trait_maps_columns() {
    let t: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
    let p = TimeParam {
        created_start: Some(t),
        created_end: Some(t),
        updated_start: Some(t),
        updated_end: Some(t),
    };

    let (q, _) = Condition::new_all()
        .add_time::<users::Column>(&p)
        .build(users::Entity::find());
    let sql = q.build(DbBackend::Sqlite).to_string();
    assert!(sql.contains("created_at"));
    assert!(sql.contains("updated_at"));
}

#[partial_entity(users::Model)]
#[derive(Debug, PartialEq, Eq)]
struct UserRsp {
    id: HyUuid,
    name: String,
    created_at: DateTimeUtc,
    updated_at: DateTimeUtc,
}

#[test]
fn normal_partial_entity_from_model() {
    let model = sample("alice");
    let rsp: UserRsp = model.clone().into();
    assert_eq!(rsp.id, model.id);
    assert_eq!(rsp.name, model.name);
    assert_eq!(rsp.created_at, model.created_at);
    assert_eq!(rsp.updated_at, model.updated_at);
}

// ---- boundary ----

#[test]
fn boundary_before_save_insert_preserves_preset_id() {
    let db = mock_connection();
    for id in [HyUuid::new(), HyUuid::nil()] {
        let am = users::ActiveModel {
            id: ActiveValue::Set(id),
            ..active("x")
        };
        let saved = block_on(am.before_save(&db, true)).unwrap();
        assert!(matches!(saved.id, ActiveValue::Set(v) if v == id));
    }
}

#[test]
fn boundary_before_save_update_keeps_not_set_fields() {
    let db = mock_connection();
    let saved = block_on(active("x").before_save(&db, false)).unwrap();
    assert!(matches!(saved.id, ActiveValue::NotSet));
    assert!(matches!(saved.created_at, ActiveValue::NotSet));
    assert!(matches!(saved.updated_at, ActiveValue::Set(_)));
}
