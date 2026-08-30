//! Default logger setup for Actix Cloud applications.
//!
//! The output is normalized for readability:
//! - Target: only keeps the first path before `::` (e.g. `project::module1::module2` => `project`).
//! - Filename: only keeps the path starting from the last `src` (e.g. `/a/b/src/c/d/main.rs` => `src/c/d/main.rs`).

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
