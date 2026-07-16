use std::path::{Path, PathBuf};

use helixflow_gateway::{ArtifactContent, ArtifactKind, ArtifactPayload};

use crate::{RunError, RunResult};

pub(crate) fn default_artifact_root() -> PathBuf {
    std::env::temp_dir().join("helixflow-run-artifacts")
}

pub(crate) async fn persist_provider_artifact(
    root: &Path,
    run_id: &str,
    step_id: &str,
    node_id: &str,
    payload: &ArtifactPayload,
) -> RunResult<String> {
    match &payload.content {
        ArtifactContent::InlineBytes { bytes, ext_hint } => {
            validate_artifact_bytes(payload, bytes)?;
            let extension = extension_for_payload(payload, ext_hint.as_deref());
            let relative = artifact_relative_path(run_id, step_id, node_id, &extension);
            write_artifact_file(root, &relative, bytes).await?;
            Ok(relative.to_string_lossy().into_owned())
        }
        ArtifactContent::RemoteUrl { url } => {
            let parsed = reqwest::Url::parse(url).map_err(|_| {
                RunError::ArtifactPersistence("remote artifact URL is invalid".to_owned())
            })?;
            if parsed.scheme() != "https" {
                return Err(RunError::ArtifactPersistence(
                    "remote artifact URL must use https".to_owned(),
                ));
            }
            let response = reqwest::get(parsed).await.map_err(|err| {
                RunError::ArtifactPersistence(format!("remote artifact download failed: {err}"))
            })?;
            if !response.status().is_success() {
                return Err(RunError::ArtifactPersistence(format!(
                    "remote artifact download returned HTTP {}",
                    response.status().as_u16()
                )));
            }
            let bytes = response.bytes().await.map_err(|err| {
                RunError::ArtifactPersistence(format!("remote artifact body failed: {err}"))
            })?;
            validate_artifact_bytes(payload, &bytes)?;
            let extension = extension_for_payload(payload, None);
            let relative = artifact_relative_path(run_id, step_id, node_id, &extension);
            write_artifact_file(root, &relative, &bytes).await?;
            Ok(relative.to_string_lossy().into_owned())
        }
        ArtifactContent::None => match payload.kind {
            ArtifactKind::Image | ArtifactKind::Video => Err(invalid_media_error(
                &payload.mime,
                "media payload has no bytes to validate",
            )),
            ArtifactKind::Text | ArtifactKind::Json => Ok(payload.storage_uri.clone()),
        },
    }
}

fn validate_artifact_bytes(payload: &ArtifactPayload, bytes: &[u8]) -> RunResult<()> {
    let parsed_mime = payload.mime.parse::<mime::Mime>().map_err(|_| {
        RunError::ArtifactPersistence("artifact MIME is invalid or unsupported".to_owned())
    })?;
    match payload.kind {
        ArtifactKind::Image if parsed_mime.type_() != mime::IMAGE => Err(invalid_media_error(
            &payload.mime,
            "artifact kind and MIME do not match",
        )),
        ArtifactKind::Image if parsed_mime.subtype() == mime::PNG => {
            validate_png(bytes).map_err(|reason| invalid_media_error("image/png", reason))
        }
        ArtifactKind::Image if parsed_mime.subtype() == mime::JPEG => {
            validate_jpeg(bytes).map_err(|reason| invalid_media_error("image/jpeg", reason))
        }
        ArtifactKind::Image => Err(invalid_media_error(
            &payload.mime,
            "image MIME has no configured validator",
        )),
        ArtifactKind::Video if parsed_mime.type_() != mime::VIDEO => Err(invalid_media_error(
            &payload.mime,
            "artifact kind and MIME do not match",
        )),
        ArtifactKind::Video if parsed_mime.subtype().as_str() == "mp4" => {
            validate_mp4(bytes).map_err(|reason| invalid_media_error("video/mp4", reason))
        }
        ArtifactKind::Video => Err(invalid_media_error(
            &payload.mime,
            "video MIME has no configured validator",
        )),
        ArtifactKind::Text | ArtifactKind::Json => Ok(()),
    }
}

fn invalid_media_error(mime: &str, reason: &str) -> RunError {
    RunError::ArtifactPersistence(format!("invalid {mime} artifact content: {reason}"))
}

fn validate_png(bytes: &[u8]) -> Result<(), &'static str> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIGNATURE) {
        return Err("missing PNG signature");
    }

    let mut cursor = SIGNATURE.len();
    let mut chunk_index = 0usize;
    let mut saw_idat = false;
    let mut saw_iend = false;
    while cursor < bytes.len() {
        let header_end = cursor.checked_add(8).ok_or("PNG chunk size overflow")?;
        if header_end > bytes.len() {
            return Err("truncated PNG chunk header");
        }
        let length = u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .map_err(|_| "invalid PNG chunk length")?,
        ) as usize;
        let chunk_type = &bytes[cursor + 4..header_end];
        let data_end = header_end
            .checked_add(length)
            .ok_or("PNG chunk size overflow")?;
        let chunk_end = data_end.checked_add(4).ok_or("PNG chunk size overflow")?;
        if chunk_end > bytes.len() {
            return Err("truncated PNG chunk data");
        }
        let expected_crc = u32::from_be_bytes(
            bytes[data_end..chunk_end]
                .try_into()
                .map_err(|_| "invalid PNG chunk CRC")?,
        );
        if png_crc32(&bytes[cursor + 4..data_end]) != expected_crc {
            return Err("PNG chunk CRC mismatch");
        }

        if chunk_index == 0 {
            if chunk_type != b"IHDR" || length != 13 {
                return Err("first PNG chunk must be a 13-byte IHDR");
            }
            let width = u32::from_be_bytes(
                bytes[header_end..header_end + 4]
                    .try_into()
                    .map_err(|_| "invalid PNG width")?,
            );
            let height = u32::from_be_bytes(
                bytes[header_end + 4..header_end + 8]
                    .try_into()
                    .map_err(|_| "invalid PNG height")?,
            );
            if width == 0 || height == 0 {
                return Err("PNG dimensions must be non-zero");
            }
        } else if chunk_type == b"IHDR" {
            return Err("PNG contains multiple IHDR chunks");
        }

        if chunk_type == b"IDAT" && length > 0 {
            saw_idat = true;
        }
        if chunk_type == b"IEND" {
            if length != 0 {
                return Err("PNG IEND chunk must be empty");
            }
            saw_iend = true;
            if chunk_end != bytes.len() {
                return Err("PNG has trailing bytes after IEND");
            }
        }
        cursor = chunk_end;
        chunk_index += 1;
    }

    if !saw_idat {
        return Err("PNG is missing image data");
    }
    if !saw_iend {
        return Err("PNG is missing IEND");
    }
    Ok(())
}

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn validate_jpeg(bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() < 4 || !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9]) {
        return Err("JPEG is missing start or end marker");
    }
    Ok(())
}

fn validate_mp4(bytes: &[u8]) -> Result<(), &'static str> {
    let mut cursor = 0usize;
    let mut box_index = 0usize;
    let mut saw_ftyp = false;
    let mut saw_moov = false;
    let mut saw_mdat = false;
    while cursor < bytes.len() {
        let parsed = parse_mp4_box(bytes, cursor)?;
        if box_index == 0 && parsed.kind != *b"ftyp" {
            return Err("first MP4 box must be ftyp");
        }
        match &parsed.kind {
            b"ftyp" => {
                if saw_ftyp {
                    return Err("MP4 contains multiple ftyp boxes");
                }
                validate_ftyp(parsed.payload)?;
                saw_ftyp = true;
            }
            b"moov" => {
                if parsed.payload.is_empty()
                    || !mp4_container_has_box(parsed.payload, *b"mvhd")?
                    || !mp4_container_has_box(parsed.payload, *b"trak")?
                {
                    return Err("MP4 moov box is missing movie or track metadata");
                }
                saw_moov = true;
            }
            b"mdat" => {
                if parsed.payload.is_empty() {
                    return Err("MP4 mdat box is empty");
                }
                saw_mdat = true;
            }
            _ => {}
        }
        cursor = parsed.end;
        box_index += 1;
    }

    if !saw_ftyp || !saw_moov || !saw_mdat {
        return Err("MP4 is missing ftyp, moov, or mdat box");
    }
    Ok(())
}

struct ParsedMp4Box<'a> {
    kind: [u8; 4],
    payload: &'a [u8],
    end: usize,
}

fn parse_mp4_box(bytes: &[u8], offset: usize) -> Result<ParsedMp4Box<'_>, &'static str> {
    let base_header_end = offset.checked_add(8).ok_or("MP4 box size overflow")?;
    if base_header_end > bytes.len() {
        return Err("truncated MP4 box header");
    }
    let size32 = u32::from_be_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .map_err(|_| "invalid MP4 box size")?,
    );
    let kind = bytes[offset + 4..base_header_end]
        .try_into()
        .map_err(|_| "invalid MP4 box type")?;
    let (header_end, end) = match size32 {
        0 => (base_header_end, bytes.len()),
        1 => {
            let extended_header_end = offset.checked_add(16).ok_or("MP4 box size overflow")?;
            if extended_header_end > bytes.len() {
                return Err("truncated extended MP4 box header");
            }
            let size64 = u64::from_be_bytes(
                bytes[base_header_end..extended_header_end]
                    .try_into()
                    .map_err(|_| "invalid extended MP4 box size")?,
            );
            let size = usize::try_from(size64).map_err(|_| "MP4 box is too large")?;
            let end = offset.checked_add(size).ok_or("MP4 box size overflow")?;
            (extended_header_end, end)
        }
        size => {
            let size = usize::try_from(size).map_err(|_| "MP4 box is too large")?;
            let end = offset.checked_add(size).ok_or("MP4 box size overflow")?;
            (base_header_end, end)
        }
    };
    if end < header_end || end > bytes.len() {
        return Err("MP4 box size exceeds content bounds");
    }
    Ok(ParsedMp4Box {
        kind,
        payload: &bytes[header_end..end],
        end,
    })
}

fn validate_ftyp(payload: &[u8]) -> Result<(), &'static str> {
    if payload.len() < 8 || (payload.len() - 8) % 4 != 0 {
        return Err("MP4 ftyp box has invalid length");
    }
    let supported = [*b"isom", *b"iso2", *b"avc1", *b"mp41", *b"mp42", *b"qt  "];
    let major_brand = payload[0..4]
        .try_into()
        .map_err(|_| "MP4 ftyp major brand is invalid")?;
    let compatible = payload[8..]
        .chunks_exact(4)
        .any(|brand| supported.iter().any(|supported| brand == supported));
    if !supported.contains(&major_brand) && !compatible {
        return Err("MP4 ftyp has no supported brand");
    }
    Ok(())
}

fn mp4_container_has_box(bytes: &[u8], expected: [u8; 4]) -> Result<bool, &'static str> {
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let parsed = parse_mp4_box(bytes, cursor)?;
        if parsed.kind == expected {
            return Ok(true);
        }
        cursor = parsed.end;
    }
    Ok(false)
}

async fn write_artifact_file(root: &Path, relative: &Path, bytes: &[u8]) -> RunResult<()> {
    let full_path = root.join(relative);
    let parent = full_path
        .parent()
        .ok_or_else(|| RunError::ArtifactPersistence("artifact path has no parent".to_owned()))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|err| RunError::ArtifactPersistence(err.to_string()))?;
    tokio::fs::write(&full_path, bytes)
        .await
        .map_err(|err| RunError::ArtifactPersistence(err.to_string()))?;
    Ok(())
}

fn artifact_relative_path(run_id: &str, step_id: &str, node_id: &str, extension: &str) -> PathBuf {
    PathBuf::from("artifacts")
        .join(run_id)
        .join(format!("{step_id}-{node_id}.{extension}"))
}

fn extension_for_payload(payload: &ArtifactPayload, ext_hint: Option<&str>) -> String {
    if let Some(extension) = ext_hint.and_then(safe_extension) {
        return extension;
    }
    mime_extension(&payload.mime).unwrap_or_else(|| match payload.kind {
        ArtifactKind::Text => "txt".to_owned(),
        ArtifactKind::Image => "bin".to_owned(),
        ArtifactKind::Video => "bin".to_owned(),
        ArtifactKind::Json => "json".to_owned(),
    })
}

fn safe_extension(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_start_matches('.');
    (!trimmed.is_empty()
        && trimmed.len() <= 12
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'))
    .then(|| trimmed.to_ascii_lowercase())
}

fn mime_extension(mime_value: &str) -> Option<String> {
    let parsed = mime_value.parse::<mime::Mime>().ok()?;
    if parsed.type_() == mime::IMAGE && parsed.subtype() == mime::PNG {
        return Some("png".to_owned());
    }
    if parsed.type_() == mime::IMAGE && parsed.subtype() == mime::JPEG {
        return Some("jpg".to_owned());
    }
    if parsed.type_() == mime::VIDEO && parsed.subtype() == mime::MP4 {
        return Some("mp4".to_owned());
    }
    if parsed.type_() == mime::TEXT {
        return Some("txt".to_owned());
    }
    if parsed == mime::APPLICATION_JSON {
        return Some("json".to_owned());
    }
    None
}
