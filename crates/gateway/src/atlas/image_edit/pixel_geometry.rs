use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, RgbaImage};
use serde_json::Value;

use crate::{ProviderError, ProviderResultValue};

use super::image_processing::invalid;

pub(super) const MAX_IMAGE_EDGE: u32 = 8192;

pub(super) struct PreparedProcessingImage {
    pub(super) image: RgbaImage,
    protected_pixels: Option<Vec<bool>>,
}

pub(super) fn prepare_image(
    intent: crate::ImageProcessingIntent,
    source: &[u8],
    parameters: &Value,
) -> ProviderResultValue<PreparedProcessingImage> {
    use crate::ImageProcessingIntent;

    let image = decode_image(source)?;
    match intent {
        ImageProcessingIntent::Outpaint => {
            let padded = pad_image(&image, parameters)?;
            Ok(PreparedProcessingImage {
                image: padded,
                protected_pixels: None,
            })
        }
        ImageProcessingIntent::Inpaint => {
            let prepared = prepare_inpaint(image, parameters)?;
            let protected = prepared.pixels().map(|pixel| pixel.0[3] != 0).collect();
            Ok(PreparedProcessingImage {
                image: prepared,
                protected_pixels: Some(protected),
            })
        }
        ImageProcessingIntent::Cutout | ImageProcessingIntent::Enhance => {
            require_only_keys(parameters, &[])?;
            Ok(PreparedProcessingImage {
                image,
                protected_pixels: None,
            })
        }
        ImageProcessingIntent::Upscale => {
            require_only_keys(parameters, &["scale", "facePolicy"])?;
            upscale_scale(parameters)?;
            Ok(PreparedProcessingImage {
                image,
                protected_pixels: None,
            })
        }
    }
}

pub(super) fn restore_protected_pixels(
    output: RgbaImage,
    prepared: &PreparedProcessingImage,
) -> RgbaImage {
    let mut restored = if output.dimensions() == prepared.image.dimensions() {
        output
    } else {
        image::imageops::resize(
            &output,
            prepared.image.width(),
            prepared.image.height(),
            image::imageops::FilterType::Lanczos3,
        )
    };
    if let Some(protected) = &prepared.protected_pixels {
        for (index, (target, source)) in restored
            .pixels_mut()
            .zip(prepared.image.pixels())
            .enumerate()
        {
            if protected[index] {
                *target = *source;
            }
        }
    }
    restored
}

pub(super) fn decode_image(bytes: &[u8]) -> ProviderResultValue<RgbaImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid("source image format is invalid"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_EDGE);
    limits.max_image_height = Some(MAX_IMAGE_EDGE);
    limits.max_alloc = Some(320 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map(DynamicImage::into_rgba8)
        .map_err(|_| invalid("image could not be decoded"))
}

pub(super) fn encode_png(image: &RgbaImage) -> ProviderResultValue<Vec<u8>> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| ProviderError::InvalidResponse("image could not be encoded".to_owned()))?;
    Ok(output.into_inner())
}

pub(super) fn pad_image(image: &RgbaImage, parameters: &Value) -> ProviderResultValue<RgbaImage> {
    require_only_keys(
        parameters,
        &[
            "left", "top", "right", "bottom", "prompt", "quality", "sizeTier",
        ],
    )?;
    let left = required_u32(parameters, "left")?;
    let top = required_u32(parameters, "top")?;
    let right = required_u32(parameters, "right")?;
    let bottom = required_u32(parameters, "bottom")?;
    let border = left
        .checked_add(top)
        .and_then(|value| value.checked_add(right))
        .and_then(|value| value.checked_add(bottom))
        .ok_or_else(|| invalid("outpaint border exceeds the image limit"))?;
    if border == 0 {
        return Err(invalid("outpaint requires a non-zero border"));
    }
    let width = image
        .width()
        .checked_add(left)
        .and_then(|value| value.checked_add(right))
        .filter(|value| *value <= MAX_IMAGE_EDGE)
        .ok_or_else(|| invalid("outpaint width exceeds the image limit"))?;
    let height = image
        .height()
        .checked_add(top)
        .and_then(|value| value.checked_add(bottom))
        .filter(|value| *value <= MAX_IMAGE_EDGE)
        .ok_or_else(|| invalid("outpaint height exceeds the image limit"))?;
    let mut padded = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    image::imageops::overlay(&mut padded, image, i64::from(left), i64::from(top));
    Ok(padded)
}

pub(super) fn prepare_inpaint(
    mut image: RgbaImage,
    parameters: &Value,
) -> ProviderResultValue<RgbaImage> {
    require_only_keys(
        parameters,
        &["x", "y", "width", "height", "prompt", "quality", "sizeTier"],
    )?;
    if parameters.get("x").is_some() {
        let x = required_u32(parameters, "x")?;
        let y = required_u32(parameters, "y")?;
        let width = required_u32(parameters, "width")?;
        let height = required_u32(parameters, "height")?;
        if width == 0
            || height == 0
            || x.checked_add(width)
                .is_none_or(|right| right > image.width())
            || y.checked_add(height)
                .is_none_or(|bottom| bottom > image.height())
        {
            return Err(invalid("inpaint rectangle is outside the source image"));
        }
        for row in y..y + height {
            for column in x..x + width {
                image.get_pixel_mut(column, row).0[3] = 0;
            }
        }
    }
    if !has_transparent_and_opaque(&image) {
        return Err(invalid(
            "inpaint source must contain both a transparent selection and opaque pixels",
        ));
    }
    Ok(image)
}

pub(super) fn size_tier(parameters: &Value) -> ProviderResultValue<&str> {
    let tier = parameters
        .get("sizeTier")
        .and_then(Value::as_str)
        .unwrap_or("1k");
    match tier {
        "1k" | "2k" | "4k" => Ok(tier),
        _ => Err(invalid("sizeTier must be 1k, 2k, or 4k")),
    }
}

pub(super) fn gpt_edit_size(
    width: u32,
    height: u32,
    tier: &str,
) -> ProviderResultValue<(u32, u32)> {
    let aspect = f64::from(width) / f64::from(height.max(1));
    if !(1.0 / 3.0..=3.0).contains(&aspect) {
        return Err(invalid(
            "GPT Image 2 edit requires a canvas aspect ratio between 1:3 and 3:1",
        ));
    }
    let long_edge = match tier {
        "1k" => 1024,
        "2k" => 2048,
        "4k" => 4096,
        _ => return Err(invalid("sizeTier must be 1k, 2k, or 4k")),
    };
    let aligned = |value: f64| ((value / 16.0).round().max(1.0) as u32) * 16;
    let mut size = if width >= height {
        (long_edge, aligned(f64::from(long_edge) / aspect))
    } else {
        (aligned(f64::from(long_edge) * aspect), long_edge)
    };
    const MIN_PIXELS: f64 = 1024.0 * 1024.0;
    let pixels = f64::from(size.0) * f64::from(size.1);
    if pixels < MIN_PIXELS {
        let scale = (MIN_PIXELS / pixels).sqrt();
        let align_up = |value: f64| ((value / 16.0).ceil().max(1.0) as u32) * 16;
        size = (
            align_up(f64::from(size.0) * scale),
            align_up(f64::from(size.1) * scale),
        );
    }
    Ok(size)
}

pub(super) fn nearest_nano_aspect_ratio(width: u32, height: u32) -> &'static str {
    const RATIOS: &[(&str, f64)] = &[
        ("1:1", 1.0),
        ("3:2", 1.5),
        ("2:3", 2.0 / 3.0),
        ("3:4", 0.75),
        ("4:3", 4.0 / 3.0),
        ("4:5", 0.8),
        ("5:4", 1.25),
        ("9:16", 9.0 / 16.0),
        ("16:9", 16.0 / 9.0),
        ("21:9", 21.0 / 9.0),
    ];
    let aspect = f64::from(width) / f64::from(height.max(1));
    RATIOS
        .iter()
        .min_by(|left, right| {
            (left.1 - aspect)
                .abs()
                .partial_cmp(&(right.1 - aspect).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|ratio| ratio.0)
        .unwrap_or("1:1")
}

pub(super) fn upscale_scale(parameters: &Value) -> ProviderResultValue<u32> {
    let scale = required_u32(parameters, "scale")?;
    if matches!(scale, 2 | 4) {
        Ok(scale)
    } else {
        Err(invalid("upscale scale must be 2 or 4"))
    }
}

fn required_u32(parameters: &Value, key: &str) -> ProviderResultValue<u32> {
    parameters
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid(&format!("{key} must be a non-negative integer")))
}

fn require_only_keys(parameters: &Value, allowed: &[&str]) -> ProviderResultValue<()> {
    let object = parameters
        .as_object()
        .ok_or_else(|| invalid("parameters must be an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid(&format!("parameter `{key}` is not accepted")));
    }
    Ok(())
}

pub(super) fn has_transparent_and_opaque(image: &RgbaImage) -> bool {
    let mut transparent = false;
    let mut opaque = false;
    for pixel in image.pixels() {
        if pixel.0[3] == 0 {
            transparent = true;
        } else {
            opaque = true;
        }
        if transparent && opaque {
            return true;
        }
    }
    false
}
