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
