#![cfg(all(
    feature = "api",
    feature = "entity",
    feature = "hyuuid",
    feature = "macros",
    feature = "seaorm"
))]

//! Security regression tests: injection, parser robustness, panic/DoS
//! resistance, and the parameterization of macro-generated queries.

mod common;

use actix_cloud_extra::HyUuid;
use actix_cloud_extra::api::{
    Condition, IDsReq, IntoExpr, OptionIDsReq, PaginationParam, TimeParam,
};
use actix_cloud_extra::utils::DataUrl;
use common::{UserRepo, count_row, sample, users};
use futures::executor::block_on;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::ColumnRef;
use sea_orm::{DbBackend, MockDatabase, MockExecResult, QueryTrait, Value};

// ---- SQL injection / parameterization ----

#[test]
fn sqli_filter_eq_binds_value() {
    let payload = "x' OR 1=1 --";
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([Vec::<users::Model>::new()])
        .into_connection();

    let cond = Condition::new_all().add(users::Column::Name.eq(payload));
    let _ = block_on(UserRepo::find(&db, cond)).unwrap();

    let log = db.into_transaction_log();
    let stmt = &log[0].statements()[0];
    assert!(stmt.sql.contains("WHERE"));
    assert!(
        stmt.sql.contains('?'),
        "value should be bound: {}",
        stmt.sql
    );
    assert!(!stmt.sql.contains(payload), "payload leaked into SQL");

    let values = stmt.values.as_ref().expect("bound values");
    assert!(values.0.contains(&Value::String(Some(payload.to_owned()))));
}

#[test]
fn sqli_like_pattern_binds_escaped_value() {
    let payload = "100%_off\\' OR 1=1 --";
    // `\` is escaped first, then `%` and `_`; the pattern is wrapped in `%`.
    let expected = "%100\\%\\_off\\\\' OR 1=1 --%";

    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([Vec::<users::Model>::new()])
        .into_connection();

    let cond = Condition::new_all().add((&payload.to_owned()).like_expr(users::Column::Name));
    let _ = block_on(UserRepo::find(&db, cond)).unwrap();

    let log = db.into_transaction_log();
    let stmt = &log[0].statements()[0];
    assert!(stmt.sql.contains("LIKE"));
    assert!(stmt.sql.contains("ESCAPE"));
    assert!(!stmt.sql.contains(payload), "payload leaked into SQL");

    let values = stmt.values.as_ref().expect("bound values");
    assert!(
        values.0.contains(&Value::String(Some(expected.to_owned()))),
        "unexpected bound values: {:?}",
        values.0
    );
}

#[test]
fn sqli_sort_rejects_metacharacter_tokens() {
    let cond = Condition::new_all().parse_sort(
        "name;DROP TABLE users,--,name",
        vec![ColumnRef::from(users::Column::Name)],
    );
    assert_eq!(cond.order.len(), 1);

    let sql = cond
        .build(users::Entity::find())
        .0
        .build(DbBackend::Sqlite)
        .to_string();
    assert!(sql.contains("ORDER BY"));
    assert!(!sql.contains("DROP"));
    assert!(!sql.contains(';'));
    assert!(!sql.contains("--"));
}

#[test]
fn sqli_sort_ignores_unlisted_and_non_column_refs() {
    let cond = Condition::new_all().parse_sort(
        "id,*,-name",
        vec![
            ColumnRef::from(users::Column::Name),
            ColumnRef::Asterisk(None),
        ],
    );
    assert_eq!(cond.order.len(), 1);
    assert_eq!(cond.order[0].1, sea_orm::Order::Desc);
}

#[test]
fn sqli_ids_req_rejects_sql_shaped_uuid_strings() {
    for payload in [
        "' OR 1=1 --",
        "550e8400e29b41d4a716446655440000",
        "{550e8400-e29b-41d4-a716-446655440000}",
        "urn:uuid:550e8400-e29b-41d4-a716-446655440000",
    ] {
        let input = serde_json::json!({"id": [payload]});
        assert!(
            serde_json::from_value::<IDsReq>(input).is_err(),
            "{payload} should not parse as an ID"
        );
        let input = serde_json::json!({"id": [payload]});
        assert!(serde_json::from_value::<OptionIDsReq>(input).is_err());
    }
}

// ---- input validation / parser robustness ----

#[test]
fn validation_pagination_bounds_not_enforced_by_deserialization() {
    // Bounds are enforced by the opt-in `Validate` step, not by serde.
    let p: PaginationParam = serde_json::from_str(r#"{"page": 0, "size": 0}"#).unwrap();
    assert_eq!((p.page, p.size), (0, 0));
    let p: PaginationParam = serde_json::from_str(r#"{"size": 101}"#).unwrap();
    assert_eq!((p.page, p.size), (1, 101));
}

#[test]
fn validation_ids_duplicates_parse_but_nil_allowed() {
    let id = HyUuid::new();
    let req: IDsReq =
        serde_json::from_value(serde_json::json!({"id": [id.to_string(), id.to_string()]}))
            .unwrap();
    assert_eq!(req.id.len(), 2);

    let nil = HyUuid::nil();
    assert!(HyUuid::parse(&nil.to_string()).is_ok());
    let req: IDsReq = serde_json::from_value(serde_json::json!({"id": [nil.to_string()]})).unwrap();
    assert_eq!(req.id, vec![nil]);
}

#[test]
fn parser_hyuuid_rejects_non_rfc_formats_and_versions() {
    let long = "a".repeat(1_000_000);
    let inputs = [
        "",
        " ",
        "550e8400-e29b-41d4-a716-44665544000",   // 35
        "550e8400-e29b-41d4-a716-4466554400000", // 37
        "550e8400e29b41d4a716446655440000",      // simple
        "{550e8400-e29b-41d4-a716-446655440000}",
        "urn:uuid:550e8400-e29b-41d4-a716-446655440000",
        "550e8400-e29b-11d4-a716-446655440000", // v1
        "550e8400-e29b-71d4-a716-446655440000", // v7
        "550e8400-e29b-41d4-a716-446655440000\n",
        "550e8400-e29b-41d4-a716-44665544000\0",
        "５５０ｅ８４００-ｅ２９ｂ-４１ｄ４-ａ７１６-４４６６５５４４００００",
    ];
    for input in inputs {
        assert!(
            HyUuid::parse(input).is_err(),
            "{input:?} should be rejected"
        );
    }
    assert!(HyUuid::parse(&long).is_err());

    // Documented current policy: nil and non-RFC variants are accepted.
    assert!(HyUuid::parse("00000000-0000-0000-0000-000000000000").is_ok());
    assert!(HyUuid::parse("550e8400-e29b-41d4-0716-446655440000").is_ok());
}

#[test]
fn parser_dataurl_rejects_malformed_frames() {
    for input in [
        "no-comma-here",
        "a,b,c",
        "data:image/png;base64,!!!!",
        "data:image/png;base64,--__",
        "data:image/png;base64,iVBOR w0KGgo=",
        "data:image/png;base64,",
        "x,AQID",
        ",",
    ] {
        assert!(DataUrl::decode(input).is_none(), "{input:?} should fail");
    }
}

#[test]
fn parser_dataurl_mime_is_content_derived() {
    // The declared prefix/scheme never controls the returned MIME type.
    for input in [
        "data:text/html;base64,iVBORw0KGgo=",
        "http://evil.example/x,iVBORw0KGgo=",
    ] {
        let url = DataUrl::decode(input).expect("payload decodes");
        assert_eq!(url.mime.mime_type(), "image/png");
    }
}

#[test]
fn parser_time_param_rejects_invalid_rfc3339() {
    for input in [
        r#"{"created_start":"2024-01-01T00:00:00"}"#,
        r#"{"created_start":"2024-01-01"}"#,
        r#"{"created_start":"2024-02-30T00:00:00Z"}"#,
        r#"{"created_start":"2024-01-01T25:00:00Z"}"#,
        r#"{"created_start":"2024-13-01T00:00:00Z"}"#,
        r#"{"created_start":"2024-01-01T00:00:00Z junk"}"#,
        r#"{"created_start":123}"#,
        r#"{"created_start":true}"#,
    ] {
        assert!(
            serde_json::from_str::<TimeParam>(input).is_err(),
            "{input} should fail"
        );
    }
}

// ---- panic / denial of service ----

#[test]
fn safety_select_page_zero_size_returns_err() {
    // F1: unvalidated size=0 must return an error, not panic in SeaORM.
    let db = MockDatabase::new(DbBackend::Sqlite).into_connection();
    let cond = Condition::new_all().add_page(PaginationParam { page: 1, size: 0 });

    let result = block_on(cond.select_page(users::Entity::find(), &db));
    assert!(result.is_err(), "size=0 should return Err");
}

#[test]
fn safety_select_page_u64_max_page_returns_err() {
    // F2: the page offset would overflow u64; reject it instead of panicking
    // (debug) or wrapping (release).
    let db = MockDatabase::new(DbBackend::Sqlite).into_connection();
    let cond = Condition::new_all().add_page(PaginationParam {
        page: u64::MAX,
        size: 100,
    });

    let result = block_on(cond.select_page(users::Entity::find(), &db));
    assert!(result.is_err(), "overflowing page should return Err");
}

#[test]
fn safety_repo_find_rejects_invalid_pagination() {
    // The generated `find` goes through the same safety checks.
    let db = MockDatabase::new(DbBackend::Sqlite).into_connection();
    let cond = Condition::new_all().add_page(PaginationParam { page: 1, size: 0 });
    assert!(block_on(UserRepo::find(&db, cond)).is_err());
}

#[test]
fn dos_pagination_split_saturates() {
    let page = PaginationParam {
        page: u64::MAX,
        size: 100,
    }
    .split(vec![1, 2, 3]);
    assert_eq!(page.total, 3);
    assert!(page.data.is_empty());

    let page = PaginationParam { page: 1, size: 0 }.split(vec![1, 2, 3]);
    assert_eq!(page.total, 3);
    assert!(page.data.is_empty());

    let page = PaginationParam {
        page: 0,
        size: u64::MAX,
    }
    .split(vec![1, 2, 3]);
    // page 0 saturates to offset 0 and the huge size is capped by the input.
    assert_eq!(page.total, 3);
    assert_eq!(page.data.len(), 3);
}

#[test]
fn dos_long_sort_and_like_inputs_do_not_panic() {
    let tokens = (0..10_000)
        .map(|i| format!("t{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let cond = Condition::new_all().parse_sort(&tokens, vec![ColumnRef::from(users::Column::Name)]);
    assert!(cond.order.is_empty());
    let sql = cond
        .build(users::Entity::find())
        .0
        .build(DbBackend::Sqlite)
        .to_string();
    assert!(sql.contains("SELECT"));

    let payload = "z".repeat(100 * 1024);
    let cond = Condition::new_all().add((&payload).like_expr(users::Column::Name));
    let sql = cond
        .build(users::Entity::find())
        .0
        .build(DbBackend::Sqlite)
        .to_string();
    assert!(sql.contains("LIKE"));

    // F4 liveness only: `decode` allocates per comma but must not panic.
    let comma_bomb = ",".repeat(100_000);
    assert!(DataUrl::decode(&comma_bomb).is_none());
}

// ---- macro-generated queries ----

#[test]
fn macro_find_by_id_parameterizes_id() {
    let row = sample("a");
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([vec![row.clone()]])
        .into_connection();

    let found = block_on(UserRepo::find_by_id(&db, &row.id)).unwrap();
    assert_eq!(found, Some(row.clone()));

    let log = db.into_transaction_log();
    let stmt = &log[0].statements()[0];
    assert!(stmt.sql.contains("WHERE"));
    assert!(stmt.sql.contains("\"id\" = ?"), "{}", stmt.sql);
    assert!(!stmt.sql.contains(&row.id.to_string()));

    let values = stmt.values.as_ref().expect("bound values");
    assert!(values.0.contains(&Value::Uuid(Some(row.id.0))));
}

#[test]
fn macro_delete_parameterizes_id_and_has_where() {
    let id = HyUuid::new();
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_exec_results([MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        }])
        .into_connection();

    assert_eq!(block_on(UserRepo::delete(&db, &id)).unwrap(), 1);

    let log = db.into_transaction_log();
    let stmt = &log[0].statements()[0];
    assert!(stmt.sql.starts_with("DELETE FROM"));
    assert!(stmt.sql.contains("WHERE"));
    assert!(stmt.sql.contains("\"id\" = ?"), "{}", stmt.sql);
    assert!(!stmt.sql.contains(&id.to_string()));

    let values = stmt.values.as_ref().expect("bound values");
    assert!(values.0.contains(&Value::Uuid(Some(id.0))));
}

#[test]
fn macro_count_parameterizes_filter_value() {
    let payload = "x' OR 1=1 --";
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([vec![count_row(0)]])
        .into_connection();

    let cond = Condition::new_all().add(users::Column::Name.eq(payload));
    assert_eq!(block_on(UserRepo::count(&db, cond)).unwrap(), 0);

    let log = db.into_transaction_log();
    let stmt = &log[0].statements()[0];
    assert!(stmt.sql.contains("COUNT(*)"));
    assert!(stmt.sql.contains("WHERE"));
    assert!(!stmt.sql.contains(payload));

    let values = stmt.values.as_ref().expect("bound values");
    assert!(values.0.contains(&Value::String(Some(payload.to_owned()))));
}

#[test]
fn macro_delete_all_has_no_where_policy() {
    // F13 characterization: the generated `delete_all` is unfiltered.
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_exec_results([MockExecResult {
            last_insert_id: 0,
            rows_affected: 0,
        }])
        .into_connection();

    block_on(UserRepo::delete_all(&db)).unwrap();

    let log = db.into_transaction_log();
    let stmt = &log[0].statements()[0];
    assert!(stmt.sql.starts_with("DELETE FROM"));
    assert!(!stmt.sql.contains("WHERE"));
}

#[test]
fn macro_find_unpaged_has_no_limit_policy() {
    // F3 characterization: no pagination means the whole table is loaded.
    let db = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results([vec![sample("a")]])
        .into_connection();

    block_on(UserRepo::find(&db, Condition::new_all())).unwrap();

    let log = db.into_transaction_log();
    let stmt = &log[0].statements()[0];
    assert!(!stmt.sql.contains("LIMIT"));
    assert!(!stmt.sql.contains("OFFSET"));
}
