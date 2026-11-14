#![cfg(all(
    feature = "api",
    feature = "entity",
    feature = "hyuuid",
    feature = "macros",
    feature = "seaorm"
))]

mod common;

use actix_cloud_extra::HyUuid;
use actix_cloud_extra::api::{Condition, PaginationParam};
use common::{UserRepo, count_row, sample, users};
use futures::executor::block_on;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::ColumnRef;
use sea_orm::{DbBackend, MockDatabase, MockExecResult};

fn db_with_rows(rows: Vec<users::Model>) -> sea_orm::DatabaseConnection {
    MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([rows])
        .into_connection()
}

// ---- normal path ----

#[test]
fn normal_find_unpaged() {
    let db = db_with_rows(vec![sample("a"), sample("b")]);

    let (rows, total) = block_on(UserRepo::find(&db, Condition::new_all())).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(total, 2);

    let log = db.into_transaction_log();
    assert_eq!(log.len(), 1);
    assert!(!log[0].statements()[0].sql.contains("LIMIT"));
}

#[test]
fn normal_find_paged() {
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([vec![sample("a"), sample("b")]])
        .append_query_results([vec![count_row(5)]])
        .into_connection();

    let cond = Condition::new_all().add_page(PaginationParam { page: 2, size: 2 });
    let (rows, total) = block_on(UserRepo::find(&db, cond)).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(total, 5);

    let log = db.into_transaction_log();
    assert_eq!(log.len(), 2);
    assert!(log[0].statements()[0].sql.contains("LIMIT"));
    assert!(log[1].statements()[0].sql.contains("COUNT(*)"));
}

#[test]
fn normal_find_by_id_some_and_none() {
    let row = sample("a");
    let db = db_with_rows(vec![row.clone()]);

    let found = block_on(UserRepo::find_by_id(&db, &row.id)).unwrap();
    assert_eq!(found, Some(row));

    let db = db_with_rows(Vec::new());
    let missing = block_on(UserRepo::find_by_id(&db, &HyUuid::new())).unwrap();
    assert!(missing.is_none());
}

#[test]
fn normal_delete_and_delete_all_rows_affected() {
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_exec_results([
            MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            },
            MockExecResult {
                last_insert_id: 0,
                rows_affected: 4,
            },
        ])
        .into_connection();

    assert_eq!(block_on(UserRepo::delete(&db, &HyUuid::new())).unwrap(), 1);
    assert_eq!(block_on(UserRepo::delete_all(&db)).unwrap(), 4);

    let log = db.into_transaction_log();
    assert_eq!(log.len(), 2);
    for tx in &log {
        assert!(tx.statements()[0].sql.contains("DELETE FROM \"users\""));
    }
}

#[test]
fn normal_count_with_filter() {
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([vec![count_row(3)]])
        .into_connection();

    let cond = Condition::new_all().add(users::Column::Name.eq("alice"));
    assert_eq!(block_on(UserRepo::count(&db, cond)).unwrap(), 3);

    let log = db.into_transaction_log();
    let sql = &log[0].statements()[0].sql;
    assert!(sql.contains("COUNT(*)"));
    assert!(sql.contains("WHERE"));
}

#[test]
fn normal_user_defined_method_preserved() {
    assert_eq!(UserRepo::marker(), "kept");
}

// ---- boundary ----

#[test]
fn boundary_count_ignores_page() {
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([vec![count_row(7)]])
        .into_connection();

    let cond = Condition::new_all()
        .add(users::Column::Name.eq("alice"))
        .add_page(PaginationParam { page: 3, size: 2 });
    assert_eq!(block_on(UserRepo::count(&db, cond)).unwrap(), 7);

    let log = db.into_transaction_log();
    let sql = &log[0].statements()[0].sql;
    assert!(!sql.contains("LIMIT"));
    assert!(!sql.contains("OFFSET"));
}

#[test]
fn boundary_find_with_sort_sql() {
    let db = db_with_rows(vec![sample("a")]);

    let cond = Condition::new_all().parse_sort("-name", vec![ColumnRef::from(users::Column::Name)]);
    let _ = block_on(UserRepo::find(&db, cond)).unwrap();

    let log = db.into_transaction_log();
    let sql = &log[0].statements()[0].sql;
    assert!(sql.contains("ORDER BY"));
    assert!(sql.contains("\"name\" DESC"));
}

// ---- extreme ----

#[test]
fn extreme_delete_all_u64_max_rows_affected() {
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_exec_results([MockExecResult {
            last_insert_id: 0,
            rows_affected: u64::MAX,
        }])
        .into_connection();

    assert_eq!(block_on(UserRepo::delete_all(&db)).unwrap(), u64::MAX);
}
