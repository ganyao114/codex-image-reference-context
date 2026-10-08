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
    assert_eq!((artifact.width, artifact.height), (2, 3));
    let duplicate = ImageArtifact::store(directory.path(), &bytes)
        .await
        .unwrap();
    assert_eq!(artifact.path, duplicate.path);
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
    assert_eq!(cached.chars().count(), MAX_DESCRIPTION_CHARS);
    assert!(
        artifact
            .cached_description("another-model")
            .await
            .unwrap()
            .is_none()
    );
    let reference = serde_json::to_string(&artifact).unwrap();
    assert!(!reference.contains("data:image"));
    assert!(reference.len() < 1024);
}
