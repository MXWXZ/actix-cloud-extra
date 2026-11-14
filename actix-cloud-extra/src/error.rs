//! Crate-wide error type and [`Result`] alias.
//!
//! ```
//! use actix_cloud_extra::{Error, Result};
//!
//! fn parse(s: &str) -> Result<()> {
//!     actix_cloud_extra::HyUuid::parse(s)?;
//!     Ok(())
//! }
//!
//! assert!(matches!(parse("bad"), Err(Error::UuidLength)));
//! ```

/// Errors returned by fallible APIs in this crate.
///
/// The enum is `#[non_exhaustive]`: match the variants you handle and add a
/// wildcard arm. It implements [`std::error::Error`] and is `Send + Sync`, so
/// it works with actix-cloud's `ResponseError` and with `?` in downstream
/// handlers (including those still returning `anyhow::Result`).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// UUID string is not exactly 36 bytes.
    #[error("uuid error: uuid must be 36 bytes")]
    UuidLength,
    /// UUID is valid but not version 4 (the nil UUID is allowed).
    #[error("uuid error: only uuid v4 is allowed")]
    UuidVersion,
    /// UUID parsing failed; [`Display`](std::fmt::Display) and
    /// [`source`](std::error::Error::source) delegate to [`uuid::Error`].
    #[error("{0}")]
    Uuid(#[from] uuid::Error),
    /// SeaORM database error; [`Display`](std::fmt::Display) and
    /// [`source`](std::error::Error::source) delegate to [`sea_orm::DbErr`].
    #[cfg(feature = "seaorm")]
    #[error("{0}")]
    Db(#[from] sea_orm::DbErr),
    /// Pagination `size` is zero.
    #[cfg(feature = "seaorm")]
    #[error("pagination size must be at least 1")]
    PaginationSize,
    /// Pagination offset overflows `u64`.
    #[cfg(feature = "seaorm")]
    #[error("pagination offset overflows u64")]
    PaginationOverflow,
}

/// [`std::result::Result`] alias defaulting to [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    // ---- normal path ----

    #[test]
    fn display_messages_are_stable() {
        assert_eq!(
            Error::UuidLength.to_string(),
            "uuid error: uuid must be 36 bytes"
        );
        assert_eq!(
            Error::UuidVersion.to_string(),
            "uuid error: only uuid v4 is allowed"
        );
        #[cfg(feature = "seaorm")]
        {
            assert_eq!(
                Error::PaginationSize.to_string(),
                "pagination size must be at least 1"
            );
            assert_eq!(
                Error::PaginationOverflow.to_string(),
                "pagination offset overflows u64"
            );
        }
    }

    #[test]
    fn uuid_source_is_exposed() {
        let inner = uuid::Uuid::parse_str("not-a-uuid").unwrap_err();
        let msg = inner.to_string();
        let err = Error::from(inner);

        assert!(matches!(err, Error::Uuid(_)));
        assert_eq!(err.to_string(), msg);
        assert!(std::error::Error::source(&err).is_some());
    }

    #[cfg(feature = "seaorm")]
    #[test]
    fn db_source_is_exposed() {
        let inner = sea_orm::DbErr::Custom("boom".to_owned());
        let msg = inner.to_string();
        let err = Error::from(inner);

        assert!(matches!(err, Error::Db(_)));
        assert_eq!(err.to_string(), msg);
        assert!(std::error::Error::source(&err).is_some());
    }

    // ---- boundary ----

    #[test]
    fn error_is_send_sync_and_boxable() {
        fn assert_error<T: std::error::Error + Send + Sync + 'static>() {}
        fn assert_boxable<T: Into<Box<dyn std::error::Error + Send + Sync>>>() {}

        assert_error::<Error>();
        assert_boxable::<Error>();
    }
}
