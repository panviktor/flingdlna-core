//! Subtitle detection and handling for DLNA playback
//!
//! This module provides utilities for:
//! - Finding subtitle files for video files
//! - Generating DIDL-Lite metadata with subtitle tags (Samsung-compatible)

use std::path::{Path, PathBuf};

/// Supported subtitle extensions in order of preference
const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "vtt", "sub", "ass", "ssa"];

/// Find a subtitle file for a given video file
///
/// Looks for subtitle files with the same name as the video file
/// but with subtitle extensions (.srt, .vtt, .sub, .ass, .ssa)
///
/// # Example
/// ```
/// use std::path::Path;
/// use dlna_controller::subtitles::find_subtitle_for_video;
///
/// let video = Path::new("/media/movie.mp4");
/// // Will look for /media/movie.srt, /media/movie.vtt, etc.
/// let subtitle = find_subtitle_for_video(video);
/// ```
pub fn find_subtitle_for_video(video_path: &Path) -> Option<PathBuf> {
    let stem = video_path.file_stem()?;
    let parent = video_path.parent()?;

    // Check each subtitle extension in order of preference
    for ext in SUBTITLE_EXTENSIONS {
        let subtitle_path = parent.join(format!("{}.{}", stem.to_string_lossy(), ext));
        if subtitle_path.exists() {
            return Some(subtitle_path);
        }
    }

    // Also check for language-specific subtitles (e.g., movie.en.srt, movie.ru.srt)
    for ext in SUBTITLE_EXTENSIONS {
        for lang in &["en", "ru", "eng", "rus", ""] {
            let subtitle_name = if lang.is_empty() {
                format!("{}.{}", stem.to_string_lossy(), ext)
            } else {
                format!("{}.{}.{}", stem.to_string_lossy(), lang, ext)
            };
            let subtitle_path = parent.join(subtitle_name);
            if subtitle_path.exists() {
                return Some(subtitle_path);
            }
        }
    }

    None
}

/// Get all subtitle files for a video
pub fn find_all_subtitles_for_video(video_path: &Path) -> Vec<PathBuf> {
    let mut subtitles = Vec::new();

    let stem = match video_path.file_stem() {
        Some(s) => s.to_string_lossy().to_string(),
        None => return subtitles,
    };

    let parent = match video_path.parent() {
        Some(p) => p,
        None => return subtitles,
    };

    // Check for all subtitle files that match the video name
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if let Some(ext_str) = ext.to_str() {
                    if SUBTITLE_EXTENSIONS.contains(&ext_str.to_lowercase().as_str()) {
                        // Check if subtitle name starts with video name
                        if let Some(name) = path.file_stem() {
                            let name_str = name.to_string_lossy();
                            if name_str == stem || name_str.starts_with(&format!("{stem}.")) {
                                subtitles.push(path);
                            }
                        }
                    }
                }
            }
        }
    }

    subtitles
}

/// Get the MIME type for a subtitle file
pub fn subtitle_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("srt") => "text/srt",
        Some("vtt") => "text/vtt",
        Some("sub") => "text/sub",
        Some("ass") | Some("ssa") => "text/x-ass",
        _ => "text/plain",
    }
}

/// Generate DIDL-Lite metadata with subtitle support (Samsung-compatible)
///
/// This generates XML metadata that includes Samsung-specific subtitle tags
/// (`sec:CaptionInfoEx`, `sec:CaptionInfo`) for better TV compatibility.
pub fn generate_didl_with_subtitle(
    video_uri: &str,
    video_mime: &str,
    video_title: &str,
    subtitle_uri: Option<&str>,
) -> String {
    let subtitle_tags = match subtitle_uri {
        Some(sub_uri) => format!(
            r#"<res protocolInfo="http-get:*:text/srt:*">{}</res>
        <sec:CaptionInfoEx sec:type="srt">{}</sec:CaptionInfoEx>
        <sec:CaptionInfo sec:type="srt">{}</sec:CaptionInfo>"#,
            xml_escape(sub_uri),
            xml_escape(sub_uri),
            xml_escape(sub_uri)
        ),
        None => String::new(),
    };

    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/"
            xmlns:dc="http://purl.org/dc/elements/1.1/"
            xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"
            xmlns:sec="http://www.sec.co.kr/">
    <item id="0" parentID="-1" restricted="1">
        <dc:title>{}</dc:title>
        <upnp:class>object.item.videoItem</upnp:class>
        <res protocolInfo="http-get:*:{}:DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000">{}</res>
        {}
    </item>
</DIDL-Lite>"#,
        xml_escape(video_title),
        video_mime,
        xml_escape(video_uri),
        subtitle_tags
    )
}

/// Escape special characters for XML
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_find_subtitle() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("movie.mp4");
        let srt_path = dir.path().join("movie.srt");

        // Create empty files
        File::create(&video_path).unwrap();
        File::create(&srt_path).unwrap();

        let found = find_subtitle_for_video(&video_path);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), srt_path);
    }

    #[test]
    fn test_find_subtitle_no_match() {
        let dir = tempdir().unwrap();
        let video_path = dir.path().join("movie.mp4");

        // Create only video file
        File::create(&video_path).unwrap();

        let found = find_subtitle_for_video(&video_path);
        assert!(found.is_none());
    }

    #[test]
    fn test_subtitle_mime_type() {
        assert_eq!(subtitle_mime_type(Path::new("sub.srt")), "text/srt");
        assert_eq!(subtitle_mime_type(Path::new("sub.vtt")), "text/vtt");
        assert_eq!(subtitle_mime_type(Path::new("sub.ass")), "text/x-ass");
    }

    #[test]
    fn test_didl_with_subtitle() {
        let didl = generate_didl_with_subtitle(
            "http://192.168.1.1:9000/movie.mp4",
            "video/mp4",
            "Test Movie",
            Some("http://192.168.1.1:9000/movie.srt"),
        );

        assert!(didl.contains("sec:CaptionInfoEx"));
        assert!(didl.contains("sec:CaptionInfo"));
        assert!(didl.contains("http://192.168.1.1:9000/movie.srt"));
    }

    #[test]
    fn test_didl_without_subtitle() {
        let didl = generate_didl_with_subtitle(
            "http://192.168.1.1:9000/movie.mp4",
            "video/mp4",
            "Test Movie",
            None,
        );

        assert!(!didl.contains("sec:CaptionInfoEx"));
        assert!(didl.contains("Test Movie"));
    }
}
