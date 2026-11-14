//! REST API helpers: pagination parameters, dynamic query condition building
//! and common request payloads.

use std::cmp;
#[cfg(feature = "seaorm")]
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::hash::Hash;

use anyhow::Result;
#[cfg(feature = "seaorm")]
use sea_orm::{
    ExprTrait as _, FromQueryResult, Order, QueryOrder as _,
    entity::prelude::async_trait::async_trait,
    prelude::*,
    sea_query::{ColumnRef, LikeExpr, SimpleExpr},
};
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

use crate::HyUuid;
#[cfg(feature = "entity")]
use crate::entity::DefaultColumnTrait;
#[cfg(feature = "seaorm")]
use crate::utils::StringUtil as _;

/// Standard paginated response: total item count plus one page of data.
#[derive(Serialize, Debug)]
pub struct PageData<T> {
    /// Total number of items across all pages.
    pub total: u64,
    /// Items of the requested page.
    pub data: Vec<T>,
}

impl<T> PageData<T> {
    /// Build from a `(data, total)` tuple, e.g. as returned by
    /// `Condition::select_page` or [`PaginationParam::split`].
    pub fn new(data: (Vec<T>, u64)) -> Self {
        Self {
            total: data.1,
            data: data.0,
        }
    }
}

/// Pagination parameters (`page` / `size`) accepted from query or body.
///
/// Both fields fall back to defaults (page 1, size 10) when missing. Call
/// `validate` (from `validator`) to enforce `page >= 1` and
/// `1 <= size <= 100`.
///
/// [`SelectPage::select_page`] additionally rejects `size == 0` and offsets
/// that would overflow `u64`, even when `validate` was not called.
#[derive(Deserialize, Validate, Clone, Debug)]
#[serde(default)]
pub struct PaginationParam {
    /// 1-based page index.
    #[validate(range(min = 1))]
    pub page: u64,

    /// Number of items per page, capped at 100 by validation.
    #[validate(range(min = 1, max = 100))]
    pub size: u64,
}

impl Default for PaginationParam {
    fn default() -> Self {
        Self { page: 1, size: 10 }
    }
}

impl PaginationParam {
    /// New pagination params using the defaults (page 1, size 10).
    pub fn new() -> Self {
        Self::default()
    }

    /// 0-based index of the first item of the page (saturating).
    pub fn left(&self) -> u64 {
        self.page.saturating_sub(1).saturating_mul(self.size)
    }

    /// Exclusive 0-based end index of the page (saturating).
    pub fn right(&self) -> u64 {
        self.page.saturating_mul(self.size)
    }

    /// Clamp a `u64` offset/count down to [`usize`] so it can be used for
    /// in-memory slicing on any platform.
    pub fn round(p: u64) -> usize {
        cmp::min(p, usize::MAX as u64) as usize
    }

    /// Apply the pagination to an in-memory list, returning the requested page
    /// plus the total item count.
    pub fn split<T>(&self, data: Vec<T>) -> PageData<T> {
        let cnt = data.len();
        PageData::new((
            data.into_iter()
                .skip(Self::round(self.left()))
                .take(Self::round(self.size))
                .collect(),
            cnt as u64,
        ))
    }
}

/// Extension for SeaORM `Select`: execute a query with optional pagination and
/// get back `(items, total_count)`.
#[cfg(feature = "seaorm")]
#[async_trait]
pub trait SelectPage<E, C>
where
    E: EntityTrait,
    C: ConnectionTrait,
{
    /// Fetch one page when `p` is given, or the whole result set (with its
    /// length as total) otherwise.
    ///
    /// Without `p` the entire result set is loaded into memory; request-facing
    /// callers should always attach a [`PaginationParam`] and enforce their own
    /// upper bound (for example HTTP query/body limits).
    ///
    /// `size == 0` and offsets that would overflow `u64` return an error
    /// instead of panicking or wrapping.
    async fn select_page(self, db: &C, p: Option<PaginationParam>) -> Result<(Vec<E::Model>, u64)>;
}

#[cfg(feature = "seaorm")]
#[async_trait]
impl<M, E, C> SelectPage<E, C> for Select<E>
where
    E: EntityTrait<Model = M>,
    M: FromQueryResult + Sized + Send + Sync,
    C: ConnectionTrait,
{
    async fn select_page(self, db: &C, p: Option<PaginationParam>) -> Result<(Vec<E::Model>, u64)> {
        if let Some(page) = p {
            // SeaORM asserts `page_size != 0` and multiplies
            // `page_size * page_index` without checks; reject both here so
            // unvalidated request parameters cannot panic or wrap.
            if page.size == 0 {
                anyhow::bail!("pagination size must be at least 1");
            }
            let page_index = page.page.saturating_sub(1);
            if page_index.checked_mul(page.size).is_none() {
                anyhow::bail!("pagination offset overflows u64");
            }
            let q = self.paginate(db, page.size);
            Ok((q.fetch_page(page_index).await?, q.num_items().await?))
        } else {
            let res = self.all(db).await?;
            let cnt = res.len() as u64;
            Ok((res, cnt))
        }
    }
}

/// Helpers to turn strings into SeaORM/SeaQuery expressions.
#[cfg(feature = "seaorm")]
pub trait IntoExpr {
    /// Build `col LIKE '%<self>%'`, escaping `\`, `%` and `_` in `self`.
    ///
    /// The pattern is bound as a parameter, not interpolated. The search term
    /// is not length-limited and a leading wildcard can force a full scan;
    /// callers should bound the term length and prefer indexed search where
    /// possible.
    fn like_expr<T>(&self, col: T) -> SimpleExpr
    where
        T: ColumnTrait;
}

#[cfg(feature = "seaorm")]
impl IntoExpr for &String {
    fn like_expr<T>(&self, col: T) -> SimpleExpr
    where
        T: ColumnTrait,
    {
        sea_orm::prelude::Expr::col((col.entity_name(), col))
            .like(LikeExpr::new(format!("%{}%", self.like_escape())).escape('\\'))
    }
}

/// The query condition hub: filters + optional pagination + sort order.
///
/// Built fluently, then applied to a SeaORM `Select` via [`Condition::build`]
/// or executed directly with [`Condition::select_page`]. All CRUD helpers
/// generated by the `default_repo` macro funnel through it.
///
/// Pagination is optional and defaults to `None`. Without [`Condition::add_page`],
/// [`Condition::select_page`] loads the entire result set into memory; callers
/// serving untrusted requests should always attach a [`PaginationParam`] and
/// enforce their own upper bound.
#[cfg(feature = "seaorm")]
pub struct Condition {
    /// Optional SQL pagination.
    pub page: Option<PaginationParam>,
    /// WHERE filters, `all()` by default.
    pub cond: sea_orm::Condition,
    /// Sort expressions, applied in insertion order.
    pub order: Vec<(SimpleExpr, Order)>,
}

#[cfg(feature = "seaorm")]
impl Default for Condition {
    fn default() -> Self {
        Self {
            cond: sea_orm::Condition::all(),
            page: None,
            order: Vec::new(),
        }
    }
}

#[cfg(feature = "seaorm")]
impl From<sea_orm::Condition> for Condition {
    fn from(cond: sea_orm::Condition) -> Self {
        Self::new(cond)
    }
}

#[cfg(feature = "seaorm")]
impl Condition {
    /// Wrap an existing [`sea_orm::Condition`] as filters.
    pub fn new(cond: sea_orm::Condition) -> Self {
        Self {
            cond,
            page: None,
            order: Vec::new(),
        }
    }

    /// New condition matching everything (`all()`).
    pub fn new_all() -> Self {
        Self::new(Self::all())
    }

    /// New condition matching anything (`any()`).
    pub fn new_any() -> Self {
        Self::new(Self::any())
    }

    /// Append one sort expression; later calls take precedence for ties.
    pub fn add_sort(mut self, col: SimpleExpr, sort: Order) -> Self {
        self.order.push((col, sort));
        self
    }

    /// Attach SQL pagination.
    pub fn add_page(mut self, page: PaginationParam) -> Self {
        self.page = Some(page);
        self
    }

    /// Parse the sort string when present, see [`Condition::parse_sort`].
    pub fn parse_sort_option(mut self, str: &Option<String>, col: Vec<ColumnRef>) -> Self {
        if let Some(str) = str {
            self = self.parse_sort(str, col)
        }
        self
    }

    /// Parse a sort string like `-created_at,+name` into sort expressions.
    ///
    /// `-`/`+` prefixes select descending/ascending order (no prefix =
    /// ascending); names not present in the `col` whitelist are silently
    /// ignored, so this is safe to feed straight from user input. Column refs
    /// without a column name (such as `ColumnRef::Asterisk`) are ignored too.
    ///
    /// The input is not length-limited; callers should bound the query-string
    /// or body size.
    pub fn parse_sort(mut self, str: &str, col: Vec<ColumnRef>) -> Self {
        let mut allow: BTreeMap<String, ColumnRef> = col
            .into_iter()
            .filter_map(|x| {
                let name = x.column()?.to_string();
                Some((name, x))
            })
            .collect();
        for i in str.split(',') {
            let order = if i.starts_with('-') {
                Order::Desc
            } else {
                Order::Asc
            };
            let name = i
                .strip_prefix('+')
                .or_else(|| i.strip_prefix('-'))
                .unwrap_or(i);
            if let Some(x) = allow.remove(name) {
                self = self.add_sort(SimpleExpr::Column(x), order);
            }
        }
        self
    }

    /// Shortcut of [`sea_orm::Condition::any`].
    pub fn any() -> sea_orm::Condition {
        sea_orm::Condition::any()
    }

    /// Shortcut of [`sea_orm::Condition::all`].
    pub fn all() -> sea_orm::Condition {
        sea_orm::Condition::all()
    }

    /// AND one more condition.
    #[allow(clippy::should_implement_trait)]
    pub fn add<C>(mut self, condition: C) -> Self
    where
        C: Into<sea_orm::Condition>,
    {
        self.cond = self.cond.add(condition.into());
        self
    }

    /// AND one more condition, ignored when `None`.
    pub fn add_option<C>(mut self, condition: Option<C>) -> Self
    where
        C: Into<sea_orm::Condition>,
    {
        self.cond = self.cond.add_option(condition);
        self
    }

    /// Apply the sort order and filters to a `Select`.
    ///
    /// Returns the built query together with the detached pagination, ready
    /// for [`SelectPage::select_page`].
    pub fn build<E>(self, mut q: Select<E>) -> (Select<E>, Option<PaginationParam>)
    where
        E: EntityTrait,
    {
        for i in self.order {
            q = q.order_by(i.0, i.1);
        }
        (q.filter(self.cond), self.page)
    }

    /// [`Condition::build`] + [`SelectPage::select_page`] in one call;
    /// returns `(items, total_count)`.
    ///
    /// Without a page attached via [`Condition::add_page`], the entire result
    /// set is loaded into memory. See [`SelectPage::select_page`] for the
    /// safety checks on `size` and the offset.
    pub async fn select_page<M, E, C>(self, q: Select<E>, db: &C) -> Result<(Vec<E::Model>, u64)>
    where
        E: EntityTrait<Model = M>,
        M: FromQueryResult + Sized + Send + Sync,
        C: ConnectionTrait,
    {
        let (q, page) = self.build(q);
        q.select_page(db, page).await
    }

    /// Add created/updated time range filters from [`TimeParam`], using the
    /// columns provided by [`DefaultColumnTrait`] (usually implemented by the
    /// `entity_timestamp` macro).
    #[cfg(feature = "entity")]
    pub fn add_time<T>(mut self, p: &TimeParam) -> Self
    where
        T: DefaultColumnTrait,
    {
        self = self.add_option(p.created_start.map(|x| T::get_created_at().gte(x)));
        self = self.add_option(p.created_end.map(|x| T::get_created_at().lte(x)));
        self = self.add_option(p.updated_start.map(|x| T::get_updated_at().gte(x)));
        self = self.add_option(p.updated_end.map(|x| T::get_updated_at().lte(x)));
        self
    }
}

/// `validator` helper checking that a list contains no duplicate items.
///
/// The list length is not capped; callers must enforce a maximum at the
/// request-body/transport layer.
///
/// # Errors
/// Will return `Err` when `x` has duplicate items.
pub fn unique_validator<T: Eq + Hash>(x: &[T]) -> Result<(), ValidationError> {
    let mut seen = HashSet::with_capacity(x.len());
    if x.iter().all(|i| seen.insert(i)) {
        Ok(())
    } else {
        Err(ValidationError::new("not unique"))
    }
}

/// Request payload carrying a list of unique IDs.
///
/// The list length is not capped and duplicates are only rejected when
/// `Validate::validate` is called. Enforce a maximum list size at the
/// request-body layer and always validate before processing.
#[derive(Debug, Validate, Deserialize)]
pub struct IDsReq {
    /// Entity IDs, must not contain duplicates.
    #[validate(custom(function = "unique_validator"))]
    pub id: Vec<HyUuid>,
}

/// Request payload carrying an optional list of unique IDs.
///
/// The list length is not capped and duplicates are only rejected when
/// `Validate::validate` is called. Enforce a maximum list size at the
/// request-body layer and always validate before processing.
#[derive(Debug, Validate, Deserialize)]
pub struct OptionIDsReq {
    /// Entity IDs, must not contain duplicates.
    #[validate(custom(function = "unique_validator"))]
    pub id: Option<Vec<HyUuid>>,
}

/// Created/updated time range filters, consumed by
/// [`Condition::add_time`].
///
/// All values are UTC datetimes, the same type the `entity_timestamp`
/// macro stores, and deserialize from RFC 3339 *strings* like
/// `2024-01-01T00:00:00Z` (a timezone offset is required; non-UTC offsets
/// are converted to UTC).
#[cfg(feature = "seaorm")]
#[derive(Debug, Deserialize, Validate)]
pub struct TimeParam {
    /// `created_at >= created_start` filter.
    pub created_start: Option<DateTimeUtc>,
    /// `created_at <= created_end` filter.
    pub created_end: Option<DateTimeUtc>,

    /// `updated_at >= updated_start` filter.
    pub updated_start: Option<DateTimeUtc>,
    /// `updated_at <= updated_end` filter.
    pub updated_end: Option<DateTimeUtc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- normal path ----

    #[test]
    fn page_data_new() {
        let page = PageData::new((vec![1, 2, 3], 7));
        assert_eq!(page.total, 7);
        assert_eq!(page.data, vec![1, 2, 3]);
    }

    #[test]
    fn page_data_serialize() {
        let value = serde_json::to_value(PageData::new((vec!["a"], 1))).unwrap();
        assert_eq!(value, serde_json::json!({"total": 1, "data": ["a"]}));
    }

    #[test]
    fn normal_page_data_serialize_empty() {
        let s = serde_json::to_string(&PageData::new((Vec::<u8>::new(), 0))).unwrap();
        assert_eq!(s, r#"{"total":0,"data":[]}"#);
    }

    #[test]
    fn pagination_default() {
        let p = PaginationParam::default();
        assert_eq!((p.page, p.size), (1, 10));
        assert_eq!((p.left(), p.right()), (0, 10));

        let p = PaginationParam::new();
        assert_eq!((p.page, p.size), (1, 10));
    }

    #[test]
    fn pagination_deserialize() {
        let p: PaginationParam = serde_json::from_str(r#"{"page": 2, "size": 20}"#).unwrap();
        assert_eq!((p.page, p.size), (2, 20));
        // Missing fields fall back to defaults.
        let p: PaginationParam = serde_json::from_str("{}").unwrap();
        assert_eq!((p.page, p.size), (1, 10));
        // String numbers are rejected.
        assert!(serde_json::from_str::<PaginationParam>(r#"{"page": "2"}"#).is_err());
    }

    #[test]
    fn normal_pagination_deserialize_partial_fields() {
        let p: PaginationParam = serde_json::from_str(r#"{"page": 5}"#).unwrap();
        assert_eq!((p.page, p.size), (5, 10));
        let p: PaginationParam = serde_json::from_str(r#"{"size": 20}"#).unwrap();
        assert_eq!((p.page, p.size), (1, 20));
        // Unknown fields are ignored.
        let p: PaginationParam =
            serde_json::from_str(r#"{"page": 2, "size": 20, "extra": true}"#).unwrap();
        assert_eq!((p.page, p.size), (2, 20));
    }

    #[test]
    fn normal_pagination_validate_accepts_legal_range() {
        for (page, size) in [(1, 1), (5, 100), (u64::MAX, 100)] {
            assert!(
                PaginationParam { page, size }.validate().is_ok(),
                "page={page} size={size}"
            );
        }
    }

    #[test]
    fn pagination_validate() {
        // Deserialization alone does not reject out-of-range values.
        let p: PaginationParam = serde_json::from_str(r#"{"page": 0}"#).unwrap();
        assert!(p.validate().is_err());
        let p: PaginationParam = serde_json::from_str(r#"{"size": 101}"#).unwrap();
        assert!(p.validate().is_err());
        let p: PaginationParam = serde_json::from_str(r#"{"page": 5, "size": 100}"#).unwrap();
        assert!(p.validate().is_ok());
    }

    #[test]
    fn pagination_split() {
        let p = PaginationParam { page: 2, size: 3 };
        let page = p.split(vec![1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(page.total, 7);
        assert_eq!(page.data, vec![4, 5, 6]);
    }

    #[test]
    fn normal_pagination_split_pages() {
        let first = PaginationParam { page: 1, size: 3 }.split(vec![1, 2, 3, 4]);
        assert_eq!(first.data, vec![1, 2, 3]);
        let last = PaginationParam { page: 2, size: 3 }.split(vec![1, 2, 3, 4]);
        assert_eq!(last.data, vec![4]);
        let empty = PaginationParam::default().split(Vec::<i32>::new());
        assert_eq!(empty.total, 0);
        assert!(empty.data.is_empty());
    }

    #[test]
    fn unique_validator_works() {
        assert!(unique_validator(&Vec::<u8>::new()).is_ok());
        assert!(unique_validator(&[1, 2, 3]).is_ok());
        assert!(unique_validator(&[1, 2, 2]).is_err());
    }

    #[test]
    fn normal_unique_validator_distinct() {
        assert!(unique_validator(&[1]).is_ok());
        assert!(unique_validator(&["a", "b", "c"]).is_ok());
    }

    #[test]
    fn ids_req_validate() {
        let a = HyUuid::new();
        let b = HyUuid::new();

        let req: IDsReq =
            serde_json::from_value(serde_json::json!({ "id": [a.to_string(), b.to_string()] }))
                .unwrap();
        assert!(req.validate().is_ok());

        // Duplicates pass deserialization but fail validation.
        let req: IDsReq =
            serde_json::from_value(serde_json::json!({ "id": [a.to_string(), a.to_string()] }))
                .unwrap();
        assert!(req.validate().is_err());
    }

    #[test]
    fn option_ids_req_validate() {
        let a = HyUuid::new();

        // Missing or null ids skip validation.
        let req: OptionIDsReq = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(req.id.is_none());
        assert!(req.validate().is_ok());
        let req: OptionIDsReq = serde_json::from_value(serde_json::json!({ "id": null })).unwrap();
        assert!(req.validate().is_ok());

        let req: OptionIDsReq =
            serde_json::from_value(serde_json::json!({ "id": [a.to_string()] })).unwrap();
        assert!(req.validate().is_ok());
        let req: OptionIDsReq =
            serde_json::from_value(serde_json::json!({ "id": [a.to_string(), a.to_string()] }))
                .unwrap();
        assert!(req.validate().is_err());
    }

    #[test]
    fn normal_ids_req_empty_list() {
        let req: IDsReq = serde_json::from_value(serde_json::json!({"id": []})).unwrap();
        assert!(req.id.is_empty());
        assert!(req.validate().is_ok());
    }

    #[test]
    fn normal_ids_req_accepts_nil() {
        let nil = HyUuid::nil();
        let req: IDsReq =
            serde_json::from_value(serde_json::json!({"id": [nil.to_string()]})).unwrap();
        assert_eq!(req.id, vec![nil]);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn normal_option_ids_empty_and_missing() {
        let req: OptionIDsReq = serde_json::from_value(serde_json::json!({"id": []})).unwrap();
        assert_eq!(req.id, Some(Vec::new()));
        assert!(req.validate().is_ok());
    }

    // ---- boundary ----

    #[test]
    fn pagination_bounds_saturate() {
        // `page: 0` (only reachable without validation) must not underflow.
        let p = PaginationParam { page: 0, size: 10 };
        assert_eq!((p.left(), p.right()), (0, 0));

        // Huge values saturate instead of overflowing.
        let p = PaginationParam {
            page: u64::MAX,
            size: 100,
        };
        assert_eq!(p.left(), u64::MAX);
        assert_eq!(p.right(), u64::MAX);
        assert_eq!(PaginationParam::round(u64::MAX), usize::MAX);
        assert_eq!(PaginationParam::round(42), 42);

        // An out-of-range window yields an empty page instead of a panic.
        let page = p.split(vec![1, 2, 3]);
        assert_eq!(page.total, 3);
        assert!(page.data.is_empty());
    }

    #[test]
    fn boundary_pagination_validation_rejects_out_of_range() {
        for (page, size) in [(0, 10), (1, 0), (1, 101), (0, 0), (1, u64::MAX)] {
            assert!(
                PaginationParam { page, size }.validate().is_err(),
                "page={page} size={size}"
            );
        }
    }

    #[test]
    fn boundary_pagination_deserialize_rejects_bad_types() {
        for input in [
            r#"{"page": null}"#,
            r#"{"page": -1}"#,
            r#"{"page": 2.5}"#,
            r#"{"page": true}"#,
            r#"{"page": []}"#,
            r#"{"page": "2"}"#,
            r#"{"size": -1}"#,
        ] {
            assert!(
                serde_json::from_str::<PaginationParam>(input).is_err(),
                "{input}"
            );
        }
    }

    #[test]
    fn boundary_pagination_unvalidated_values() {
        // Deserialization does not validate; page 0 / size 0 must not panic.
        let p = PaginationParam { page: 0, size: 10 };
        assert_eq!((p.left(), p.right()), (0, 0));

        let p = PaginationParam { page: 1, size: 0 };
        assert_eq!((p.left(), p.right()), (0, 0));
        let page = p.split(vec![1, 2, 3]);
        assert_eq!(page.total, 3);
        assert!(page.data.is_empty());

        let p = PaginationParam { page: 5, size: 2 };
        let page = p.split(vec![1, 2, 3]);
        assert_eq!(page.total, 3);
        assert!(page.data.is_empty());
    }

    #[test]
    fn boundary_unique_validator_duplicates_and_error() {
        let err = unique_validator(&[1, 2, 1]).unwrap_err();
        assert_eq!(err.code.as_ref(), "not unique");
        assert!(unique_validator(&["a", "a"]).is_err());
    }

    #[test]
    fn boundary_ids_req_invalid() {
        assert!(serde_json::from_value::<IDsReq>(serde_json::json!({})).is_err());
        assert!(serde_json::from_value::<IDsReq>(serde_json::json!({"id": null})).is_err());
        assert!(serde_json::from_value::<IDsReq>(serde_json::json!({"id": "x"})).is_err());
        assert!(serde_json::from_value::<IDsReq>(serde_json::json!({"id": [1]})).is_err());
        for bad in [
            "not-a-uuid",
            "550e8400e29b41d4a716446655440000",
            "{550e8400-e29b-41d4-a716-446655440000}",
            "urn:uuid:550e8400-e29b-41d4-a716-446655440000",
        ] {
            let v = serde_json::json!({"id": [bad]});
            assert!(serde_json::from_value::<IDsReq>(v).is_err(), "{bad}");
        }

        let a = HyUuid::new();
        let req: IDsReq =
            serde_json::from_value(serde_json::json!({"id": [a.to_string(), a.to_string()]}))
                .unwrap();
        assert!(req.validate().is_err());
    }

    #[test]
    fn boundary_option_ids_invalid() {
        assert!(serde_json::from_value::<OptionIDsReq>(serde_json::json!({"id": "x"})).is_err());
        let a = HyUuid::new();
        let req: OptionIDsReq =
            serde_json::from_value(serde_json::json!({"id": [a.to_string(), a.to_string()]}))
                .unwrap();
        assert!(req.validate().is_err());
    }

    // ---- extreme ----

    #[test]
    fn extreme_pagination_u64_max() {
        let input = format!(r#"{{"page": {}, "size": 100}}"#, u64::MAX);
        let p: PaginationParam = serde_json::from_str(&input).unwrap();
        assert!(p.validate().is_ok());
        let page = p.split(vec![1, 2, 3]);
        assert_eq!(page.total, 3);
        assert!(page.data.is_empty());
    }

    // ---- SeaORM-dependent tests ----

    #[cfg(feature = "seaorm")]
    mod seaorm_tests {
        use std::collections::BTreeMap;

        use futures::executor::block_on;
        use sea_orm::sea_query::ColumnRef;
        use sea_orm::{DbBackend, MockDatabase, QueryFilter, QueryTrait, Value};

        use super::*;

        mod notes {
            use sea_orm::entity::prelude::*;

            #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
            #[sea_orm(table_name = "notes")]
            pub struct Model {
                #[sea_orm(primary_key, auto_increment = false)]
                pub id: i64,
                pub name: String,
                pub created_at: DateTimeUtc,
                pub updated_at: DateTimeUtc,
            }

            #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
            pub enum Relation {}

            impl ActiveModelBehavior for ActiveModel {}
        }

        #[cfg(feature = "entity")]
        impl crate::entity::DefaultColumnTrait for notes::Column {
            fn get_created_at() -> impl ColumnTrait {
                Self::CreatedAt
            }

            fn get_updated_at() -> impl ColumnTrait {
                Self::UpdatedAt
            }
        }

        fn ts(s: &str) -> DateTimeUtc {
            s.parse().unwrap()
        }

        fn sample(name: &str) -> notes::Model {
            notes::Model {
                id: 1,
                name: name.to_owned(),
                created_at: ts("2024-01-01T00:00:00Z"),
                updated_at: ts("2024-01-01T00:00:00Z"),
            }
        }

        fn count_row(n: i64) -> BTreeMap<String, Value> {
            BTreeMap::from([("num_items".to_owned(), Value::BigInt(Some(n)))])
        }

        fn col(c: notes::Column) -> ColumnRef {
            ColumnRef::from(c)
        }

        fn expr(c: notes::Column) -> SimpleExpr {
            SimpleExpr::Column(ColumnRef::from(c))
        }

        // ---- normal path ----

        #[test]
        fn normal_time_param_offsets_and_empty() {
            let p: TimeParam = serde_json::from_str("{}").unwrap();
            assert!(p.created_start.is_none());
            assert!(p.created_end.is_none());
            assert!(p.updated_start.is_none());
            assert!(p.updated_end.is_none());
            assert!(p.validate().is_ok());

            let p: TimeParam = serde_json::from_str(
                r#"{"created_start":"2024-01-01T12:00:00+08:00","updated_end":"2024-01-01T00:00:00Z"}"#,
            )
            .unwrap();
            assert_eq!(p.created_start.unwrap(), ts("2024-01-01T04:00:00Z"));
            assert_eq!(p.updated_end.unwrap(), ts("2024-01-01T00:00:00Z"));
        }

        #[test]
        fn normal_parse_sort_asc_desc() {
            let cols = vec![col(notes::Column::Name), col(notes::Column::CreatedAt)];
            let cond = Condition::new_all().parse_sort("+name,-created_at", cols);
            assert_eq!(cond.order.len(), 2);
            assert_eq!(cond.order[0].1, Order::Asc);
            assert_eq!(cond.order[1].1, Order::Desc);

            let cond = Condition::new_all().parse_sort("name", vec![col(notes::Column::Name)]);
            assert_eq!(cond.order.len(), 1);
            assert_eq!(cond.order[0].1, Order::Asc);
        }

        #[test]
        fn normal_parse_sort_option() {
            let cols = || vec![col(notes::Column::Name)];
            let cond = Condition::new_all().parse_sort_option(&None, cols());
            assert!(cond.order.is_empty());
            let cond = Condition::new_all().parse_sort_option(&Some("name".to_owned()), cols());
            assert_eq!(cond.order.len(), 1);
        }

        #[test]
        fn normal_parse_sort_whitelist() {
            let cond =
                Condition::new_all().parse_sort("name,unknown", vec![col(notes::Column::Name)]);
            assert_eq!(cond.order.len(), 1);
        }

        #[test]
        fn normal_add_sort_and_add_page_build() {
            let cond = Condition::new_all()
                .add_sort(expr(notes::Column::Name), Order::Asc)
                .add_sort(expr(notes::Column::CreatedAt), Order::Desc)
                .add_page(PaginationParam { page: 2, size: 5 });
            let (q, page) = cond.build(notes::Entity::find());
            let page = page.expect("pagination should be detached by build");
            assert_eq!((page.page, page.size), (2, 5));

            let sql = q.build(DbBackend::Sqlite).to_string();
            assert!(sql.contains("ORDER BY"));
            assert!(sql.contains("\"name\" ASC"));
            assert!(sql.contains("\"created_at\" DESC"));
        }

        #[test]
        fn normal_condition_build_filter_sort_sql() {
            let (q, page) = Condition::new_all()
                .add(notes::Column::Name.eq("alice"))
                .add_option(None::<sea_orm::Condition>)
                .build(notes::Entity::find());
            assert!(page.is_none());
            let sql = q.build(DbBackend::Sqlite).to_string();
            assert!(sql.contains("WHERE"));
            assert!(sql.contains("\"name\""));
        }

        #[cfg(feature = "entity")]
        #[test]
        fn normal_add_time_sql() {
            let t = ts("2024-01-01T00:00:00Z");
            let p = TimeParam {
                created_start: Some(t),
                created_end: Some(t),
                updated_start: Some(t),
                updated_end: Some(t),
            };
            let (q, _) = Condition::new_all()
                .add_time::<notes::Column>(&p)
                .build(notes::Entity::find());
            let sql = q.build(DbBackend::Sqlite).to_string();
            assert!(sql.contains("created_at"));
            assert!(sql.contains("updated_at"));

            let empty = TimeParam {
                created_start: None,
                created_end: None,
                updated_start: None,
                updated_end: None,
            };
            let base = Condition::new_all()
                .build(notes::Entity::find())
                .0
                .build(DbBackend::Sqlite)
                .to_string();
            let with_empty = Condition::new_all()
                .add_time::<notes::Column>(&empty)
                .build(notes::Entity::find())
                .0
                .build(DbBackend::Sqlite)
                .to_string();
            assert_eq!(base, with_empty, "empty TimeParam should add no filter");
        }

        #[test]
        fn normal_select_page_paged_mock() {
            let db = MockDatabase::new(DbBackend::Sqlite)
                .append_query_results([vec![sample("a"), sample("b")]])
                .append_query_results([vec![count_row(5)]])
                .into_connection();

            let (rows, total) = block_on(
                Condition::new_all()
                    .add_page(PaginationParam { page: 2, size: 2 })
                    .select_page(notes::Entity::find(), &db),
            )
            .unwrap();
            assert_eq!(rows, vec![sample("a"), sample("b")]);
            assert_eq!(total, 5);

            let log = db.into_transaction_log();
            let sql: Vec<_> = log
                .iter()
                .flat_map(|t| t.statements().iter().map(|s| s.sql.clone()))
                .collect();
            assert_eq!(sql.len(), 2);
            assert!(sql[0].contains("LIMIT"));
            assert!(sql[1].contains("COUNT(*)"));
        }

        #[test]
        fn normal_select_page_unpaged_mock() {
            let db = MockDatabase::new(DbBackend::Sqlite)
                .append_query_results([vec![sample("a"), sample("b")]])
                .into_connection();

            let (rows, total) =
                block_on(Condition::new_all().select_page(notes::Entity::find(), &db)).unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(total, 2);
        }

        #[test]
        fn normal_like_expr_escaping() {
            let pattern = "50%_off\\".to_owned();
            let sql = notes::Entity::find()
                .filter((&pattern).like_expr(notes::Column::Name))
                .build(DbBackend::Sqlite)
                .to_string();
            assert!(sql.contains("LIKE"));
            assert!(sql.contains("ESCAPE"));
        }

        // ---- boundary ----

        #[test]
        fn boundary_parse_sort_edge_tokens() {
            let cols = || vec![col(notes::Column::Name), col(notes::Column::CreatedAt)];
            assert!(Condition::new_all().parse_sort("", cols()).order.is_empty());
            assert!(
                Condition::new_all()
                    .parse_sort("+,-", cols())
                    .order
                    .is_empty()
            );
            assert!(
                Condition::new_all()
                    .parse_sort(" name", cols())
                    .order
                    .is_empty()
            );
            assert!(
                Condition::new_all()
                    .parse_sort("+ name", cols())
                    .order
                    .is_empty()
            );
            assert!(
                Condition::new_all()
                    .parse_sort("NAME", cols())
                    .order
                    .is_empty()
            );
            // Duplicate names: the first occurrence wins, later ones are ignored.
            let cond = Condition::new_all().parse_sort("name,-name", cols());
            assert_eq!(cond.order.len(), 1);
            assert_eq!(cond.order[0].1, Order::Asc);
        }

        #[test]
        fn boundary_parse_sort_skips_non_column_refs() {
            // Refs without a column name (e.g. `*`) are ignored, not a panic.
            let cond = Condition::new_all().parse_sort("anything", vec![ColumnRef::Asterisk(None)]);
            assert!(cond.order.is_empty());
        }

        #[test]
        fn boundary_time_param_rejects_naive_and_invalid_values() {
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
                assert!(serde_json::from_str::<TimeParam>(input).is_err(), "{input}");
            }
        }

        #[test]
        fn boundary_time_param_separator_and_precision() {
            // RFC 3339 allows a space separator; chrono also accepts lowercase z.
            let p: TimeParam =
                serde_json::from_str(r#"{"created_start":"2024-01-01 12:00:00Z"}"#).unwrap();
            assert!(p.created_start.is_some());
            let p: TimeParam =
                serde_json::from_str(r#"{"created_start":"2024-01-01T12:00:00z"}"#).unwrap();
            assert!(p.created_start.is_some());
            // More than nanosecond precision is accepted (truncated by chrono).
            let p: TimeParam =
                serde_json::from_str(r#"{"created_start":"2024-01-01T00:00:00.123456789123Z"}"#)
                    .unwrap();
            assert!(p.created_start.is_some());
        }

        #[test]
        fn boundary_select_page_unvalidated_page_zero() {
            let db = MockDatabase::new(DbBackend::Sqlite)
                .append_query_results([vec![sample("a")]])
                .append_query_results([vec![count_row(1)]])
                .into_connection();

            let (rows, total) = block_on(
                Condition::new_all()
                    .add_page(PaginationParam { page: 0, size: 1 })
                    .select_page(notes::Entity::find(), &db),
            )
            .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(total, 1);

            let log = db.into_transaction_log();
            assert!(log[0].statements()[0].sql.contains("LIMIT"));
        }

        // ---- extreme ----

        #[test]
        fn extreme_time_param_offset_rollover_and_leap_second() {
            // +08:00 midnight rolls back to the previous UTC day.
            let p: TimeParam =
                serde_json::from_str(r#"{"created_start":"2024-01-01T00:00:00+08:00"}"#).unwrap();
            assert_eq!(p.created_start.unwrap(), ts("2023-12-31T16:00:00Z"));

            // Leap-second input is accepted (chrono normalizes it internally).
            let p: TimeParam =
                serde_json::from_str(r#"{"created_start":"2016-12-31T23:59:60Z"}"#).unwrap();
            assert!(p.created_start.is_some());
        }

        #[test]
        fn extreme_parse_sort_long_token_list() {
            let tokens = (0..10_000)
                .map(|i| format!("unknown{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let cond = Condition::new_all().parse_sort(&tokens, vec![col(notes::Column::Name)]);
            assert!(cond.order.is_empty());
        }

        #[test]
        fn extreme_like_expr_empty_and_unicode() {
            for pattern in [String::new(), "中文%_".to_owned()] {
                let sql = notes::Entity::find()
                    .filter((&pattern).like_expr(notes::Column::Name))
                    .build(DbBackend::Sqlite)
                    .to_string();
                assert!(sql.contains("LIKE"));
            }
        }
    }
}
