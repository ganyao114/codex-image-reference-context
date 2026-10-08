use anyhow::Context;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_protocol::models::ImageReference;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::*;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event_with_timeout;
use image::ImageBuffer;
use image::Rgba;
use serde_json::json;
use tokio::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_image_is_described_once_and_only_text_survives_in_main_requests_and_rollout()
-> anyhow::Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_config(|config| {
        let _ = config.features.enable(Feature::ImageReferenceContext);
    });
    let test = builder.build(&server).await?;
    let path = test.workspace_path("blue.png");
    ImageBuffer::from_pixel(2, 3, Rgba([0u8, 0, 255, 255])).save(&path)?;
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("caption"),
                ev_assistant_message("caption-text", "A solid blue rectangle."),
                ev_completed("caption"),
            ]),
            sse(vec![
                ev_response_created("main-1"),
                ev_assistant_message("answer-1", "The reference describes a blue rectangle."),
                ev_completed("main-1"),
            ]),
            sse(vec![
                ev_response_created("main-2"),
                ev_assistant_message("answer-2", "Still blue."),
                ev_completed("main-2"),
            ]),
        ],
    )
    .await;
    for iteration in 0..2 {
        let image = if iteration == 0 {
            UserInput::LocalImage {
                path: path.clone(),
                detail: None,
            }
        } else {
            UserInput::Image {
                image: ImageReference::Inline {
                    image_url: format!(
                        "data:image/png;base64,{}",
                        STANDARD.encode(std::fs::read(&path)?)
                    ),
                },
                detail: None,
            }
        };
        test.codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![image]))
            .await?;
        wait_for_event_with_timeout(
            &test.codex,
            |e| matches!(e, EventMsg::TurnComplete(_)),
            Duration::from_secs(30),
        )
        .await;
    }
    let requests = mock.requests();
    assert_eq!(
        requests.len(),
        3,
        "the duplicate image should use its cached description"
    );
    assert!(requests[0].body_json().to_string().contains("data:image/"));
    for request in &requests[1..] {
        let body = request.body_json().to_string();
        assert!(!body.contains("data:image/"));
        assert!(!body.contains("\"type\":\"input_image\""));
        assert!(body.contains("A solid blue rectangle."));
        assert!(body.contains("image_reference"));
    }
    let artifacts = test.codex_home_path().join("image-artifacts");
    let pngs: Vec<_> = std::fs::read_dir(artifacts)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
        .collect();
    assert_eq!(pngs.len(), 1);
    assert_eq!(std::fs::read(pngs[0].path())?, std::fs::read(&path)?);
    let rollout = test.codex.rollout_path().context("rollout path")?;
    test.codex.flush_rollout().await?;
    let history = std::fs::read_to_string(rollout)?;
    assert!(
        !history.contains("data:image/"),
        "image bytes must not be persisted in the rollout"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_visual_analysis_preserves_disk_reference_and_never_falls_back_to_inline()
-> anyhow::Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_config(|config| {
        let _ = config.features.enable(Feature::ImageReferenceContext);
    });
    let test = builder.build(&server).await?;
    let path = test.workspace_path("uncaptioned.png");
    ImageBuffer::from_pixel(2, 3, Rgba([0u8, 255, 0, 255])).save(&path)?;
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("empty-caption"),
                ev_completed("empty-caption"),
            ]),
            sse(vec![
                ev_response_created("main"),
                ev_assistant_message("answer", "No visual facts are available."),
                ev_completed("main"),
            ]),
        ],
    )
    .await;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::LocalImage {
            path,
            detail: None,
        }]))
        .await?;
    wait_for_event_with_timeout(
        &test.codex,
        |e| matches!(e, EventMsg::TurnComplete(_)),
        Duration::from_secs(30),
    )
    .await;
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    let main = requests[1].body_json().to_string();
    assert!(!main.contains("data:image/"));
    assert!(main.contains("unavailable"));
    assert!(main.contains("image_reference"));
    assert!(main.contains("image-artifacts"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn view_image_returns_description_not_inline_content_to_the_next_model_request()
-> anyhow::Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_config(|config| {
        let _ = config.features.enable(Feature::ImageReferenceContext);
    });
    let test = builder.build(&server).await?;
    ImageBuffer::from_pixel(2, 3, Rgba([255u8, 0, 0, 255])).save(test.workspace_path("red.png"))?;
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("tool"),
                ev_function_call(
                    "image-call",
                    "view_image",
                    &json!({"path":"red.png"}).to_string(),
                ),
                ev_completed("tool"),
            ]),
            sse(vec![
                ev_response_created("caption"),
                ev_assistant_message("caption-text", "A solid red rectangle."),
                ev_completed("caption"),
            ]),
            sse(vec![
                ev_response_created("main"),
                ev_assistant_message("answer", "Red."),
                ev_completed("main"),
            ]),
        ],
    )
    .await;
    test.submit_turn_with_permission_profile("inspect red.png", PermissionProfile::Disabled)
        .await?;
    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].body_json().to_string().contains("data:image/"));
    let main = requests[2].body_json().to_string();
    assert!(main.contains("A solid red rectangle."));
    assert!(!main.contains("data:image/"));
    assert!(!main.contains("\"type\":\"input_image\""));
    Ok(())
}
