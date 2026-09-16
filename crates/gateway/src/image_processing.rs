use std::{collections::BTreeMap, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ProviderResultValue;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageProcessingIntent {
    Outpaint,
    Inpaint,
    Cutout,
    Upscale,
    Enhance,
}

impl ImageProcessingIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Outpaint => "outpaint",
            Self::Inpaint => "inpaint",
            Self::Cutout => "cutout",
            Self::Upscale => "upscale",
            Self::Enhance => "enhance",
        }
    }

    pub(crate) fn family(self) -> ProfileFamily {
        match self {
            Self::Outpaint | Self::Inpaint => ProfileFamily::Edit,
            Self::Cutout => ProfileFamily::Matte,
            Self::Upscale => ProfileFamily::Upscale,
            Self::Enhance => ProfileFamily::Enhance,
        }
    }

    pub(crate) fn default_profile(self) -> &'static str {
        match self {
            Self::Outpaint => "gpt-image-2",
            Self::Inpaint => "banana-2",
            Self::Cutout => "youchuan-v8.2",
            Self::Upscale => "tencent",
            Self::Enhance => "photo-cleanup",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageProcessingProfile {
    pub name: &'static str,
    pub model: &'static str,
    pub supports_quality: bool,
    pub supports_size_tier: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageProcessingCapabilities {
    pub provider: String,
    pub defaults: BTreeMap<String, String>,
    pub profiles: BTreeMap<String, Vec<ImageProcessingProfile>>,
}

#[derive(Debug, Clone)]
pub struct ImageProcessingRequest {
    pub intent: ImageProcessingIntent,
    pub profile: Option<String>,
    pub parameters: Value,
    pub source: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ImageProcessingOutput {
    pub model: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct ImageProcessingSubmission {
    pub provider_task_id: Option<String>,
    pub model: String,
    completion:
        Pin<Box<dyn Future<Output = ProviderResultValue<ImageProcessingOutput>> + Send + 'static>>,
}

impl ImageProcessingSubmission {
    pub(crate) fn new(
        provider_task_id: Option<String>,
        model: String,
        completion: impl Future<Output = ProviderResultValue<ImageProcessingOutput>> + Send + 'static,
    ) -> Self {
        Self {
            provider_task_id,
            model,
            completion: Box::pin(completion),
        }
    }

    pub async fn complete(self) -> ProviderResultValue<ImageProcessingOutput> {
        self.completion.await
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileFamily {
    Edit,
    Matte,
    Upscale,
    Enhance,
}

#[derive(Clone, Copy)]
pub(crate) struct ProfileSpec {
    pub(crate) name: &'static str,
    pub(crate) model: &'static str,
    pub(crate) family: ProfileFamily,
    pub(crate) supports_quality: bool,
    pub(crate) supports_size_tier: bool,
}

pub(crate) const IMAGE_PROCESSING_PROFILES: &[ProfileSpec] = &[
    ProfileSpec {
        name: "banana-2",
        model: "google/nano-banana-2/edit",
        family: ProfileFamily::Edit,
        supports_quality: false,
        supports_size_tier: true,
    },
    ProfileSpec {
        name: "banana-pro",
        model: "google/nano-banana-pro/edit",
        family: ProfileFamily::Edit,
        supports_quality: false,
        supports_size_tier: false,
    },
    ProfileSpec {
        name: "gpt-image-2",
        model: "openai/gpt-image-2/edit",
        family: ProfileFamily::Edit,
        supports_quality: true,
        supports_size_tier: true,
    },
    ProfileSpec {
        name: "qwen-image",
        model: "atlascloud/qwen-image/edit",
        family: ProfileFamily::Edit,
        supports_quality: false,
        supports_size_tier: false,
    },
    ProfileSpec {
        name: "youchuan-v8.2",
        model: "youchuan/v8.2/remove-background",
        family: ProfileFamily::Matte,
        supports_quality: false,
        supports_size_tier: false,
    },
    ProfileSpec {
        name: "youchuan-v8.1",
        model: "youchuan/v8.1/remove-background",
        family: ProfileFamily::Matte,
        supports_quality: false,
        supports_size_tier: false,
    },
    ProfileSpec {
        name: "tencent",
        model: "tencent/image/upscaler",
        family: ProfileFamily::Upscale,
        supports_quality: false,
        supports_size_tier: false,
    },
    ProfileSpec {
        name: "atlas-upscaler",
        model: "atlascloud/image-upscaler",
        family: ProfileFamily::Upscale,
        supports_quality: false,
        supports_size_tier: false,
    },
    ProfileSpec {
        name: "real-esrgan",
        model: "atlascloud/real-esrgan",
        family: ProfileFamily::Upscale,
        supports_quality: false,
        supports_size_tier: false,
    },
    ProfileSpec {
        name: "photo-cleanup",
        model: "atlascloud/photo-cleanup",
        family: ProfileFamily::Enhance,
        supports_quality: false,
        supports_size_tier: false,
    },
];
