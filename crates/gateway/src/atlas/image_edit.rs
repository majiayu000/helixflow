use serde_json::{Value, json};

use super::{optional_string, wired_image_to_video_media};
use crate::{ProviderError, ProviderRequest, ProviderResultValue};

mod image_processing;
mod pixel_geometry;

pub(super) fn image_generate_body(
    req: &ProviderRequest,
    model: &str,
    prompt: &str,
) -> ProviderResultValue<Value> {
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "enable_sync_mode": true,
        "output_format": optional_string(&req.params, "output_format").unwrap_or_else(|| "png".to_owned()),
    });
    if req.capability == "image_edit" {
        let (images, _, _) = wired_image_to_video_media(&req.params)?;
        if images.is_empty() {
            return Err(ProviderError::InvalidRequest(
                "image_edit requires at least one wired image input".to_owned(),
            ));
        }
        if images.len() > 14 {
            return Err(ProviderError::InvalidRequest(
                "image_edit accepts at most 14 input images".to_owned(),
            ));
        }
        body["images"] = json!(images);
        if let Some(aspect_ratio) = optional_string(&req.params, "aspect_ratio") {
            body["aspect_ratio"] = Value::String(aspect_ratio);
        }
        return Ok(body);
    }
    body["num_images"] = json!(
        req.params
            .get("num_images")
            .and_then(Value::as_u64)
            .unwrap_or(1)
    );
    body["aspect_ratio"] =
        json!(optional_string(&req.params, "aspect_ratio").unwrap_or_else(|| "1:1".to_owned()));
    Ok(body)
}
