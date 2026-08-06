use crate::session::RendererProtocol;
use dlna_core::Renderer;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playability {
    Playable,
    Maybe,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct PlayabilityReport {
    pub playability: Playability,
    pub reason: Option<String>,
    pub protocol: RendererProtocol,
}

impl PlayabilityReport {
    fn playable(protocol: RendererProtocol) -> Self {
        Self {
            playability: Playability::Playable,
            reason: None,
            protocol,
        }
    }

    fn maybe(protocol: RendererProtocol, reason: impl Into<String>) -> Self {
        Self {
            playability: Playability::Maybe,
            reason: Some(reason.into()),
            protocol,
        }
    }

    fn unsupported(protocol: RendererProtocol, reason: impl Into<String>) -> Self {
        Self {
            playability: Playability::Unsupported,
            reason: Some(reason.into()),
            protocol,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Mp4,
    Mov,
    M4v,
    Webm,
    Mkv,
    Avi,
    MpegTs,
    Mpeg,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoCodec {
    H264,
    H265,
    Vp8,
    Vp9,
    Av1,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
struct MediaProbe {
    container: Container,
    video_codec: Option<VideoCodec>,
    has_video: bool,
}

pub fn assess_file(renderer: &Renderer, path: &Path) -> PlayabilityReport {
    let protocol = protocol_from_renderer(renderer);
    match protocol {
        RendererProtocol::Dlna => PlayabilityReport::playable(protocol),
        RendererProtocol::Chromecast => {
            let probe = probe_file(path);
            assess_chromecast(probe, None)
        }
    }
}

pub fn assess_request(renderer: &Renderer, request: &crate::PlayRequest) -> PlayabilityReport {
    let protocol = protocol_from_renderer(renderer);
    match protocol {
        RendererProtocol::Dlna => PlayabilityReport::playable(protocol),
        RendererProtocol::Chromecast => {
            let mime = request.content_type.as_deref();
            let probe = probe_url(&request.url, mime);
            assess_chromecast(probe, mime)
        }
    }
}

fn protocol_from_renderer(renderer: &Renderer) -> RendererProtocol {
    match renderer.location.scheme() {
        "cast" => RendererProtocol::Chromecast,
        _ => RendererProtocol::Dlna,
    }
}

fn probe_file(path: &Path) -> MediaProbe {
    let ext = path.extension().and_then(|e| e.to_str());
    if is_audio_extension(ext) {
        return MediaProbe {
            container: container_from_extension(ext),
            video_codec: None,
            has_video: false,
        };
    }

    let container = container_from_extension(ext);
    match container {
        Container::Mp4 | Container::Mov | Container::M4v => probe_mp4(path, container),
        Container::Webm | Container::Mkv => probe_matroska(path, container),
        _ => MediaProbe {
            container,
            video_codec: None,
            has_video: true,
        },
    }
}

fn probe_url(url: &str, mime: Option<&str>) -> MediaProbe {
    let ext = extension_from_url(url);
    let is_audio =
        mime.is_some_and(|m| m.starts_with("audio/")) || is_audio_extension(ext.as_deref());

    let container =
        container_from_mime(mime).unwrap_or_else(|| container_from_extension(ext.as_deref()));

    MediaProbe {
        container,
        video_codec: None,
        has_video: !is_audio,
    }
}

fn assess_chromecast(probe: MediaProbe, mime: Option<&str>) -> PlayabilityReport {
    let protocol = RendererProtocol::Chromecast;

    if is_audio_only(&probe, mime) {
        return assess_chromecast_audio(mime, probe.container);
    }

    match probe.container {
        Container::Mp4 | Container::Mov | Container::M4v => match probe.video_codec {
            Some(VideoCodec::H264) => PlayabilityReport::playable(protocol),
            Some(VideoCodec::H265) => PlayabilityReport::unsupported(
                protocol,
                "Chromecast does not support HEVC/H.265 in MP4 containers",
            ),
            Some(VideoCodec::Vp8) => PlayabilityReport::maybe(
                protocol,
                "VP8 in MP4 is not consistently supported on Chromecast",
            ),
            Some(VideoCodec::Vp9) => PlayabilityReport::maybe(
                protocol,
                "VP9 in MP4 is not consistently supported on Chromecast",
            ),
            Some(VideoCodec::Av1) => {
                PlayabilityReport::maybe(protocol, "AV1 support varies on Chromecast")
            }
            Some(VideoCodec::Unknown) | None => PlayabilityReport::maybe(
                protocol,
                "Unknown MP4 video codec; Chromecast support is uncertain",
            ),
        },
        Container::Webm => match probe.video_codec {
            Some(VideoCodec::Vp8) | Some(VideoCodec::Vp9) => PlayabilityReport::playable(protocol),
            Some(VideoCodec::Av1) => {
                PlayabilityReport::maybe(protocol, "AV1 support varies on Chromecast")
            }
            _ => PlayabilityReport::maybe(
                protocol,
                "Unknown WebM video codec; Chromecast support is uncertain",
            ),
        },
        Container::Mkv => match probe.video_codec {
            Some(VideoCodec::H265) => PlayabilityReport::unsupported(
                protocol,
                "Chromecast does not support HEVC/H.265 in MKV containers",
            ),
            _ => PlayabilityReport::maybe(protocol, "MKV container support varies on Chromecast"),
        },
        Container::Avi | Container::Mpeg | Container::MpegTs => PlayabilityReport::unsupported(
            protocol,
            "Container format is not supported by Chromecast",
        ),
        Container::Unknown => PlayabilityReport::maybe(
            protocol,
            "Unknown container; Chromecast support is uncertain",
        ),
    }
}

fn assess_chromecast_audio(mime: Option<&str>, container: Container) -> PlayabilityReport {
    let protocol = RendererProtocol::Chromecast;
    let mime = mime.unwrap_or_default();

    if mime.starts_with("audio/") {
        let subtype = mime.trim_start_matches("audio/").to_lowercase();
        if matches!(
            subtype.as_str(),
            "mpeg" | "mp3" | "aac" | "mp4" | "ogg" | "opus" | "wav"
        ) {
            return PlayabilityReport::playable(protocol);
        }
        if subtype == "flac" {
            return PlayabilityReport::maybe(protocol, "FLAC support varies on Chromecast");
        }
        return PlayabilityReport::maybe(
            protocol,
            "Unknown audio MIME type; Chromecast support is uncertain",
        );
    }

    match container {
        Container::Mp4 | Container::Mov | Container::M4v => PlayabilityReport::playable(protocol),
        _ => PlayabilityReport::maybe(protocol, "Unknown audio container"),
    }
}

fn is_audio_only(probe: &MediaProbe, mime: Option<&str>) -> bool {
    if probe.has_video {
        return false;
    }

    if let Some(mime) = mime {
        return mime.starts_with("audio/");
    }

    matches!(
        probe.container,
        Container::Mp4 | Container::Mov | Container::M4v
    ) || matches!(probe.container, Container::Unknown)
}

fn probe_mp4(path: &Path, container: Container) -> MediaProbe {
    use std::fs::File;
    use std::io::BufReader;

    let mut has_video = false;
    let mut video_codec = None;

    let reader = match File::open(path) {
        Ok(f) => BufReader::new(f),
        Err(_) => {
            return MediaProbe {
                container,
                video_codec: None,
                has_video: true,
            };
        }
    };

    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(_) => {
            return MediaProbe {
                container,
                video_codec: None,
                has_video: true,
            };
        }
    };

    let mp4 = match mp4::Mp4Reader::read_header(reader, size) {
        Ok(m) => m,
        Err(_) => {
            return MediaProbe {
                container,
                video_codec: None,
                has_video: true,
            };
        }
    };

    for track in mp4.tracks().values() {
        if track.track_type().ok() == Some(mp4::TrackType::Video) {
            has_video = true;
            let codec = match track.media_type() {
                Ok(mp4::MediaType::H264) => Some(VideoCodec::H264),
                Ok(mp4::MediaType::H265) => Some(VideoCodec::H265),
                Ok(mp4::MediaType::VP9) => Some(VideoCodec::Vp9),
                _ => Some(VideoCodec::Unknown),
            };
            if codec.is_some() {
                video_codec = codec;
                break;
            }
        }
    }

    MediaProbe {
        container,
        video_codec,
        has_video,
    }
}

fn probe_matroska(path: &Path, container: Container) -> MediaProbe {
    let matroska = match matroska::open(path) {
        Ok(m) => m,
        Err(_) => {
            return MediaProbe {
                container,
                video_codec: None,
                has_video: true,
            };
        }
    };

    let mut has_video = false;
    let mut video_codec = None;
    for track in matroska.tracks.iter() {
        if track.is_video() {
            has_video = true;
            video_codec = Some(codec_from_matroska(track.codec_id.as_str()));
            break;
        }
    }

    MediaProbe {
        container,
        video_codec,
        has_video,
    }
}

fn codec_from_matroska(codec_id: &str) -> VideoCodec {
    let upper = codec_id.to_uppercase();
    if upper.contains("AVC") {
        VideoCodec::H264
    } else if upper.contains("HEVC") || upper.contains("HVC") {
        VideoCodec::H265
    } else if upper.contains("VP9") {
        VideoCodec::Vp9
    } else if upper.contains("VP8") {
        VideoCodec::Vp8
    } else if upper.contains("AV1") {
        VideoCodec::Av1
    } else {
        VideoCodec::Unknown
    }
}

fn container_from_mime(mime: Option<&str>) -> Option<Container> {
    let mime = mime?;
    if mime.starts_with("video/mp4") {
        Some(Container::Mp4)
    } else if mime.starts_with("video/webm") {
        Some(Container::Webm)
    } else if mime.starts_with("video/x-matroska") {
        Some(Container::Mkv)
    } else if mime.starts_with("video/quicktime") {
        Some(Container::Mov)
    } else if mime.starts_with("video/mpeg") {
        Some(Container::Mpeg)
    } else if mime.starts_with("video/mp2t") {
        Some(Container::MpegTs)
    } else {
        None
    }
}

fn container_from_extension(ext: Option<&str>) -> Container {
    match ext.unwrap_or_default().to_lowercase().as_str() {
        "mp4" => Container::Mp4,
        "m4v" => Container::M4v,
        "mov" => Container::Mov,
        "webm" => Container::Webm,
        "mkv" => Container::Mkv,
        "avi" => Container::Avi,
        "ts" | "m2ts" => Container::MpegTs,
        "mpeg" | "mpg" => Container::Mpeg,
        _ => Container::Unknown,
    }
}

fn is_audio_extension(ext: Option<&str>) -> bool {
    matches!(
        ext.unwrap_or_default().to_lowercase().as_str(),
        "mp3" | "aac" | "m4a" | "wav" | "flac" | "ogg" | "opus"
    )
}

fn extension_from_url(url: &str) -> Option<String> {
    let trimmed = url.split('?').next().unwrap_or(url);
    let trimmed = trimmed.split('#').next().unwrap_or(trimmed);
    let path = trimmed.rsplit('/').next().unwrap_or(trimmed);
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_string())
}
