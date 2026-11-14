//! SeaORM entity helpers.

use sea_orm::{ColumnTrait, FromJsonQueryResult};
use serde::{Deserialize, Serialize};

/// Wrapper of `Vec<String>` stored as a JSON array column (via
/// [`FromJsonQueryResult`]).
///
/// Useful when an entity needs a simple list field without a join table.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, FromJsonQueryResult)]
pub struct VecString(pub Vec<String>);

/// Maps an entity to its default timestamp columns.
///
/// Usually implemented automatically by the `entity_timestamp` macro, and
/// consumed by [`crate::api::Condition::add_time`] to filter on
/// creation/update time ranges.
pub trait DefaultColumnTrait {
    /// Column storing the creation timestamp.
    fn get_created_at() -> impl ColumnTrait;
    /// Column storing the last update timestamp.
    fn get_updated_at() -> impl ColumnTrait;
}

#[cfg(test)]
mod tests {
    use sea_orm::sea_query::{ArrayType, ColumnType, Nullable, Value, ValueType};
    use sea_orm::{ActiveValue, IntoActiveValue};
    use serde_json::json;

    use super::*;

    // ---- normal path ----

    #[test]
    fn normal_vec_string_serde_roundtrip() {
        let v = VecString(vec!["a".into(), "b".into()]);
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(s, r#"["a","b"]"#);
        assert_eq!(serde_json::from_str::<VecString>(&s).unwrap(), v);
    }

    #[test]
    fn normal_vec_string_default_and_eq() {
        assert_eq!(VecString::default(), VecString(Vec::new()));
        assert_eq!(VecString(vec!["a".into()]), VecString(vec!["a".into()]));
        assert_ne!(VecString(vec!["a".into()]), VecString(vec!["b".into()]));
        assert_eq!(
            VecString(vec!["a".into()]).clone(),
            VecString(vec!["a".into()])
        );
    }

    #[test]
    fn normal_value_from_json() {
        let v = VecString(vec!["a".into(), "b".into()]);
        assert_eq!(
            Value::from(v),
            Value::Json(Some(Box::new(json!(["a", "b"]))))
        );
    }

    #[test]
    fn normal_value_type_try_from_json() {
        let v = VecString(vec!["a".into()]);
        let value = Value::Json(Some(Box::new(json!(["a"]))));
        assert_eq!(<VecString as ValueType>::try_from(value).unwrap(), v);
    }

    // ---- boundary ----

    #[test]
    fn boundary_value_type_rejects_wrong_or_malformed() {
        assert!(<VecString as ValueType>::try_from(Value::Json(None)).is_err());
        assert!(<VecString as ValueType>::try_from(Value::String(Some("a".into()))).is_err());
        for malformed in [
            json!({}),
            json!("x"),
            json!(null),
            json!([1]),
            json!(["a", null]),
        ] {
            let value = Value::Json(Some(Box::new(malformed)));
            assert!(<VecString as ValueType>::try_from(value).is_err());
        }
    }

    #[test]
    fn boundary_column_and_array_type() {
        assert_eq!(<VecString as ValueType>::column_type(), ColumnType::Json);
        assert_eq!(<VecString as ValueType>::array_type(), ArrayType::Json);
    }

    #[test]
    fn boundary_type_name_and_nullable() {
        assert_eq!(<VecString as ValueType>::type_name(), "VecString");
        assert_eq!(<VecString as Nullable>::null(), Value::Json(None));
    }

    #[test]
    fn boundary_into_active_value() {
        let v = VecString(vec!["a".into()]);
        assert_eq!(v.clone().into_active_value(), ActiveValue::Set(v));
    }

    // ---- extreme ----

    #[test]
    fn extreme_vec_string_empty_unicode() {
        let v = VecString(vec![
            String::new(),
            "中文".into(),
            "line1\nline2".into(),
            "\"quoted\"".into(),
        ]);
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<VecString>(&s).unwrap(), v);

        let value = Value::from(v.clone());
        assert_eq!(<VecString as ValueType>::try_from(value).unwrap(), v);

        let empty = VecString(Vec::new());
        assert_eq!(
            <VecString as ValueType>::try_from(Value::from(empty.clone())).unwrap(),
            empty
        );
    }
}
