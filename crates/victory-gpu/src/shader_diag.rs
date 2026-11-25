//! `VICTORY_SHADER_DIAG=<dir>`: one JSONL record per pipeline classification
//! outcome (shader translation, root signature, mesh/graphics/compute
//! pipeline creation, and known partial-support shortcuts), to
//! `<dir>/shader-diag.jsonl`. Built to answer "why doesn't this draw look
//! right": each record names the shaders involved (stage + content hash,
//! the same hash `VICTORY_SHADER_SAVE` and crash reports use), a
//! classification (`ok`, `missing-shader`, `root-signature-refused`,
//! `mesh-pipeline-refused`, `pipeline-refused`, `rect-gs-fallback`,
//! `depth-bounds-unsupported`, `state-not-kept`, `device-method-unimplemented`),
//! and whatever detail was on hand at that call site (the graphics state's
//! `describe()` - input layout, render target/depth formats, blend and
//! rasterizer state - on success; the host's rejection error on failure).
//! Off by default; every call site already had an equivalent `tracing::warn!`
//! for a human reading logs live, this adds the same information in one
//! greppable/parseable file for after-the-fact analysis across a whole run.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

fn folder() -> Option<&'static std::path::PathBuf> {
    static FOLDER: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    FOLDER
        .get_or_init(|| std::env::var_os("VICTORY_SHADER_DIAG").map(std::path::PathBuf::from))
        .as_ref()
}

static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Appends one record. `shaders` and `detail` are rendered with their
/// `Debug`/`Display` form and escaped as JSON strings - plain data, not
/// pre-built JSON, so callers need not think about quoting.
pub(crate) fn record(classification: &str, shaders: impl std::fmt::Debug, detail: impl std::fmt::Display) {
    let Some(dir) = folder() else { return };
    let mut file = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if file.is_none() {
        let _ = std::fs::create_dir_all(dir);
        *file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("shader-diag.jsonl"))
            .ok();
    }
    let Some(f) = file.as_mut() else { return };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    let _ = writeln!(
        f,
        r#"{{"ts_us":{ts},"classification":"{}","shaders":"{}","detail":"{}"}}"#,
        escape(classification),
        escape(&format!("{shaders:?}")),
        escape(&detail.to_string())
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn escapes_quotes_backslashes_and_newlines() {
        assert_eq!(super::escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }
}
