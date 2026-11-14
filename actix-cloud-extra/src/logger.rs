//! Default logger setup for Actix Cloud applications.
//!
//! The output is normalized for readability:
//! - Target: only keeps the first path before `::` (e.g. `project::module1::module2` => `project`).
//! - Filename: only keeps the path starting from the last `src` (e.g. `/a/b/src/c/d/main.rs` => `src/c/d/main.rs`).
//!
//! Security note: `target` and `filename` are passed through without escaping
//! control characters. Keep untrusted data out of those fields (including the
//! reserved `_filename` override); otherwise a newline or terminal escape can
//! forge log lines or manipulate the terminal.

use std::path::PathBuf;

use actix_cloud::{
    logger::{LogItem, Logger, LoggerBuilder, LoggerGuard},
    tracing::Level,
};

/// Transform a log item before printing, see [module docs](self) for the rules.
fn transformer(mut item: LogItem) -> LogItem {
    // Trim target path.
    // Only keep the first path before `::`. Change several library targets to our own.
    item.target = item
        .target
        .split("::")
        .next()
        .unwrap_or("unknown")
        .to_owned();

    // Remove useless file path.
    // Only keep path starting from the last `src`.
    if let Some(filename) = &item.filename {
        let mut buf = Vec::new();
        let s = PathBuf::from(filename);
        for i in s.iter().rev() {
            buf.push(i);
            if i.eq_ignore_ascii_case("src") {
                break;
            }
        }
        let mut s = PathBuf::new();
        for i in buf.into_iter().rev() {
            s.push(i);
        }
        item.filename = Some(s.to_string_lossy().into());
    }
    item
}

/// Start the default logger.
///
/// - `enable`: when `false`, nothing is started and `(None, None)` is returned.
/// - `json`: output JSON lines instead of plain text.
/// - `verbose`: include file name and line number, and log at `DEBUG` level.
/// - `filter`: extra predicate, items returning `false` are dropped.
///
/// Returns the logger and a guard which stops logging when dropped; both are
/// `Some` on success and `None` when the logger is disabled.
///
/// # Panics
/// Installs a global tracing subscriber and panics if one is already installed
/// (for example when called twice or when another library initialized
/// `tracing`). Call it exactly once during startup.
///
/// The transformer does not escape control characters in `target`/`filename`;
/// keep untrusted data out of those fields (see the module docs).
pub fn start_logger(
    enable: bool,
    json: bool,
    verbose: bool,
    filter: fn(item: &LogItem) -> bool,
) -> (Option<Logger>, Option<LoggerGuard>) {
    if enable {
        let mut builder = LoggerBuilder::new();
        if json {
            builder = builder.json();
        }
        if verbose {
            builder = builder.filename().line_number().level(Level::DEBUG);
        }
        builder = builder.transformer(transformer).filter(filter);
        let (logger, guard) = builder.start();
        (Some(logger), Some(guard))
    } else {
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use actix_cloud::tracing::Level;
    use serde_json::{Map, Value};

    use super::*;

    fn item(target: &str, filename: Option<&str>) -> LogItem {
        LogItem {
            time: Value::Null,
            level: Level::INFO,
            message: String::new(),
            target: target.to_owned(),
            fields: Map::new(),
            span: Map::new(),
            filename: filename.map(str::to_owned),
            line_number: None,
        }
    }

    fn filename_of(target: &str, filename: &str) -> String {
        transformer(item(target, Some(filename)))
            .filename
            .expect("filename should stay set")
    }

    fn expected_path(parts: &[&str]) -> String {
        let mut p = PathBuf::new();
        for part in parts {
            p.push(part);
        }
        p.to_string_lossy().into_owned()
    }

    // ---- normal path ----

    #[test]
    fn normal_start_logger_disabled() {
        let (logger, guard) = start_logger(false, true, true, |_| true);
        assert!(logger.is_none());
        assert!(guard.is_none());
    }

    #[test]
    fn normal_transformer_keeps_plain_target() {
        let out = transformer(item("app", None));
        assert_eq!(out.target, "app");
        assert!(out.filename.is_none());
    }

    #[test]
    fn normal_transformer_trims_target() {
        assert_eq!(transformer(item("app::db::query", None)).target, "app");
    }

    #[test]
    fn normal_transformer_truncates_at_src() {
        assert_eq!(
            filename_of("app", "/a/b/src/c/d/main.rs"),
            expected_path(&["src", "c", "d", "main.rs"])
        );
    }

    // ---- boundary ----

    #[test]
    fn boundary_transformer_empty_and_leading_separator_target() {
        assert_eq!(transformer(item("", None)).target, "");
        // `split("::").next()` never returns None, so "unknown" is unreachable.
        assert_eq!(transformer(item("::foo", None)).target, "");
    }

    #[test]
    fn boundary_transformer_uses_last_src() {
        assert_eq!(
            filename_of("app", "/a/src/b/src/c.rs"),
            expected_path(&["src", "c.rs"])
        );
    }

    #[test]
    fn boundary_transformer_src_case_insensitive() {
        assert_eq!(
            filename_of("app", "/x/SRC/y.rs"),
            expected_path(&["SRC", "y.rs"])
        );
    }

    #[test]
    fn boundary_transformer_no_src_and_none() {
        assert_eq!(
            filename_of("app", "a/b/main.rs"),
            expected_path(&["a", "b", "main.rs"])
        );
        assert!(transformer(item("app", None)).filename.is_none());
    }

    #[test]
    fn boundary_transformer_empty_and_src_like_components() {
        assert_eq!(filename_of("app", ""), expected_path(&[]));
        assert_eq!(
            filename_of("app", "srcfile/x.rs"),
            expected_path(&["srcfile", "x.rs"])
        );
        assert_eq!(filename_of("app", "src.rs"), expected_path(&["src.rs"]));
    }

    // ---- boundary (absolute paths) ----

    #[cfg(unix)]
    #[test]
    fn boundary_transformer_keeps_absolute_path_without_src() {
        // `Path::iter()` yields the root, so an absolute path without `src`
        // survives the rebuild unchanged.
        assert_eq!(filename_of("app", "/a/b/main.rs"), "/a/b/main.rs");
    }

    // ---- security ----

    #[test]
    fn logging_transformer_preserves_printable_metadata() {
        let out = transformer(item("app::db::query", Some("src/main.rs")));
        assert_eq!(out.target, "app");
        assert_eq!(out.filename.as_deref(), Some("src/main.rs"));

        let out = transformer(item("app", Some("src/中文.rs")));
        assert_eq!(out.filename.as_deref(), Some("src/中文.rs"));
    }
}
