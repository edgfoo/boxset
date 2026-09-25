//! Number-to-string helpers

use std::path::Path;
use std::time::Duration;

pub fn filename(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// An output directory as it goes into a sentence. A relative path takes a
/// `./` so it reads as a directory rather than as a bare word.
pub fn directory(path: &Path) -> String {
    if path.as_os_str().is_empty() || path == Path::new(".") {
        return "./".to_string();
    }
    match path.is_absolute() || path.starts_with(".") || path.starts_with("..") {
        true => path.display().to_string(),
        false => format!("./{}", path.display()),
    }
}

/// `4.2s` below a minute, `1:04` above it
pub fn elapsed(duration: Duration) -> String {
    let secs = duration.as_secs_f64();
    if secs < 60.0 {
        return format!("{secs:.1}s");
    }
    let secs = secs.round() as u64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// A source's length, where whole seconds are enough.
pub fn duration(secs: f64) -> String {
    let secs = secs.round() as u64;
    if secs < 60 {
        return format!("{secs}s");
    }
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn size(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return String::new();
    };
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1}MB", bytes / MB)
    } else {
        format!("{:.0}KB", bytes / KB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_minute_elapsed_keeps_a_decimal() {
        assert_eq!(elapsed(Duration::from_millis(420)), "0.4s");
        assert_eq!(elapsed(Duration::from_millis(4200)), "4.2s");
    }

    #[test]
    fn elapsed_past_a_minute_is_clock_shaped() {
        assert_eq!(elapsed(Duration::from_secs(64)), "1:04");
    }

    #[test]
    fn size_switches_unit_at_a_megabyte() {
        assert_eq!(size(Some(1024)), "1KB");
        assert_eq!(size(Some(1024 * 1024)), "1.0MB");
    }
}
