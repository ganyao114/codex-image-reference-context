use super::*;
use image::ImageBuffer;
use image::ImageFormat;
use image::Rgba;

#[tokio::test]
async fn stores_original_bytes_and_bounded_unicode_description_without_inline_payload() {
    let directory = tempfile::tempdir().unwrap();
    let mut bytes = Vec::new();
    ImageBuffer::from_pixel(2, 3, Rgba([0u8, 0, 255, 255]))
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    let artifact = ImageArtifact::store(directory.path(), &bytes)
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(&artifact.path).await.unwrap(), bytes);
    assert_eq!(
        artifact,
        ImageArtifact {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            path: artifact.path.clone(),
            width: 2,
            height: 3,
            size_bytes: bytes.len(),
        }
    );
    let duplicate = ImageArtifact::store(directory.path(), &bytes)
        .await
        .unwrap();
    assert_eq!(artifact, duplicate);
    let description = "蓝色".repeat(MAX_DESCRIPTION_CHARS);
    artifact
        .cache_description("test-model", &description)
        .await
        .unwrap();
    let cached = artifact
        .cached_description("test-model")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cached, "蓝色".repeat(MAX_DESCRIPTION_CHARS / 2));
    assert!(
        artifact
            .cached_description("another-model")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        serde_json::to_value(&artifact).unwrap(),
        serde_json::json!({
            "sha256": artifact.sha256,
            "path": artifact.path,
            "width": 2,
            "height": 3,
            "size_bytes": bytes.len(),
        })
    );
}

#[tokio::test]
async fn rejects_corrupted_existing_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let mut bytes = Vec::new();
    ImageBuffer::from_pixel(2, 3, Rgba([0u8, 0, 255, 255]))
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    let artifact = ImageArtifact::store(directory.path(), &bytes)
        .await
        .unwrap();
    tokio::fs::write(&artifact.path, b"corrupted")
        .await
        .unwrap();
    let error = ImageArtifact::store(directory.path(), &bytes)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "local image artifact failed integrity verification"
    );
}

#[tokio::test]
async fn rejects_oversized_description_cache_before_decoding() {
    let directory = tempfile::tempdir().unwrap();
    let artifact = ImageArtifact {
        sha256: "test".to_string(),
        path: directory.path().join("test.png"),
        width: 2,
        height: 3,
        size_bytes: 0,
    };
    tokio::fs::write(
        artifact.description_path("test-model"),
        vec![b'x'; MAX_DESCRIPTION_CACHE_BYTES + 1],
    )
    .await
    .unwrap();
    let error = artifact.cached_description("test-model").await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "image description cache exceeds its size limit"
    );
}
