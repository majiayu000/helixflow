use std::path::{Component, Path, PathBuf};

use tokio::io::AsyncWriteExt;

use crate::{RunError, RunResult};

const ARTIFACT_DIRECTORY: &str = "artifacts";
const CREATE_ATTEMPTS: usize = 8;

pub(crate) struct PendingArtifact {
    relative: PathBuf,
    final_path: PathBuf,
    partial_path: PathBuf,
    file: Option<tokio::fs::File>,
    published: bool,
}

impl PendingArtifact {
    pub(crate) async fn create(root: &Path, extension: &str) -> RunResult<Self> {
        if !is_allowed_extension_component(extension) {
            return Err(path_error("artifact extension is not allowed"));
        }

        tokio::fs::create_dir_all(root)
            .await
            .map_err(|_| path_error("artifact root could not be prepared"))?;
        let canonical_root = tokio::fs::canonicalize(root)
            .await
            .map_err(|_| path_error("artifact root could not be verified"))?;
        let logical_parent = root.join(ARTIFACT_DIRECTORY);
        tokio::fs::create_dir_all(&logical_parent)
            .await
            .map_err(|_| path_error("artifact directory could not be prepared"))?;
        let canonical_parent = tokio::fs::canonicalize(&logical_parent)
            .await
            .map_err(|_| path_error("artifact directory could not be verified"))?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(path_error("artifact directory escapes the configured root"));
        }

        for _ in 0..CREATE_ATTEMPTS {
            let final_name = format!("{}.{}", uuid::Uuid::now_v7(), extension);
            let partial_name = format!(".{}.part", uuid::Uuid::now_v7());
            let relative = PathBuf::from(ARTIFACT_DIRECTORY).join(&final_name);
            validate_relative_path(&relative)?;
            let final_path = canonical_parent.join(final_name);
            if tokio::fs::try_exists(&final_path)
                .await
                .map_err(|_| path_error("artifact destination could not be checked"))?
            {
                continue;
            }
            let partial_path = canonical_parent.join(partial_name);
            match tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial_path)
                .await
            {
                Ok(file) => {
                    return Ok(Self {
                        relative,
                        final_path,
                        partial_path,
                        file: Some(file),
                        published: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(path_error("artifact partial file could not be created")),
            }
        }

        Err(path_error("artifact filename could not be allocated"))
    }

    #[cfg(test)]
    pub(crate) fn relative_path(&self) -> &Path {
        &self.relative
    }

    pub(crate) async fn write_chunk(&mut self, bytes: &[u8]) -> RunResult<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| path_error("artifact partial file is not writable"))?;
        file.write_all(bytes)
            .await
            .map_err(|_| path_error("artifact partial write failed"))
    }

    pub(crate) async fn publish(mut self) -> RunResult<PathBuf> {
        let mut file = self
            .file
            .take()
            .ok_or_else(|| path_error("artifact partial file is not writable"))?;
        file.flush()
            .await
            .map_err(|_| path_error("artifact partial flush failed"))?;
        file.sync_all()
            .await
            .map_err(|_| path_error("artifact partial sync failed"))?;
        drop(file);

        tokio::fs::hard_link(&self.partial_path, &self.final_path)
            .await
            .map_err(|_| path_error("artifact destination could not be published"))?;
        if tokio::fs::remove_file(&self.partial_path).await.is_err() {
            let _ = tokio::fs::remove_file(&self.final_path).await;
            return Err(path_error("artifact partial cleanup failed"));
        }

        self.published = true;
        Ok(self.relative.clone())
    }

    pub(crate) async fn abort(mut self) {
        self.file.take();
        let _ = tokio::fs::remove_file(&self.partial_path).await;
        self.published = true;
    }
}

impl Drop for PendingArtifact {
    fn drop(&mut self) {
        if !self.published {
            self.file.take();
            let _ = std::fs::remove_file(&self.partial_path);
        }
    }
}

pub(crate) async fn write_complete_artifact(
    root: &Path,
    extension: &str,
    bytes: &[u8],
) -> RunResult<PathBuf> {
    let mut pending = PendingArtifact::create(root, extension).await?;
    if let Err(error) = pending.write_chunk(bytes).await {
        pending.abort().await;
        return Err(error);
    }
    pending.publish().await
}

pub(crate) fn validate_relative_path(relative: &Path) -> RunResult<()> {
    let valid_component = |component: Component<'_>| match component {
        Component::Normal(value) => value.to_str().is_some_and(|value| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        }),
        _ => false,
    };
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || !relative.components().all(valid_component)
    {
        return Err(path_error("artifact relative path is not allowed"));
    }
    Ok(())
}

fn is_allowed_extension_component(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 12
        && extension
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn path_error(message: &str) -> RunError {
    RunError::ArtifactPersistence(message.to_owned())
}
