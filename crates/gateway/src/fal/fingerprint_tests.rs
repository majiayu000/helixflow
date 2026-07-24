use super::*;
use crate::Provider;

#[test]
fn changing_image_model_changes_config_fingerprint() {
    let a = FalProvider::new(FalProviderConfig::new(
        "key".to_owned(),
        "https://queue.fal.run".to_owned(),
        "fal-ai/flux/dev".to_owned(),
    ));
    let b = FalProvider::new(FalProviderConfig::new(
        "key".to_owned(),
        "https://queue.fal.run".to_owned(),
        "fal-ai/flux/schnell".to_owned(),
    ));
    assert_ne!(a.config_fingerprint("fal"), b.config_fingerprint("fal"));
    assert_eq!(a.config_fingerprint("fal"), a.config_fingerprint("fal"));
}
