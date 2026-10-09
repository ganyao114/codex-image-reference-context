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
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_feature_keeps_inline_images_without_isolated_analysis() -> anyhow::Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_config(|config| {
        let _ = config.features.disable(Feature::ImageReferenceContext);
    });
    let test = builder.build(&server).await?;
    let path = test.workspace_path("blue.png");
    ImageBuffer::from_pixel(2, 3, Rgba([0u8, 0, 255, 255])).save(&path)?;
    let mock = mount_sse_sequence(
        &server,
        vec![sse(vec![
            ev_response_created("main"),
            ev_assistant_message("answer", "Blue."),
            ev_completed("main"),
        ])],
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
    assert_eq!(requests.len(), 1);
    assert!(requests[0].body_json().to_string().contains("data:image/"));
    assert!(!test.codex_home_path().join("image-artifacts").exists());
    Ok(())
}

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
            .start_or_steer_turn(TurnInputRequest::user_input(vec![
                UserInput::Text {
                    text: "PRIVATE_MAIN_CONTEXT_DO_NOT_FORWARD".to_string(),
                    text_elements: Vec::new(),
                },
                image,
            ]))
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
    assert_eq!(
        requests[0].message_input_texts("user"),
        Vec::<String>::new()
    );
    assert!(
        requests[0]
            .body_json()
            .get("tools")
            .is_none_or(|tools| { tools.as_array().is_some_and(Vec::is_empty) })
    );
    assert!(
        !requests[0]
            .body_json()
            .to_string()
            .contains("PRIVATE_MAIN_CONTEXT_DO_NOT_FORWARD")
    );
    for request in &requests[1..] {
        let body = request.body_json().to_string();
        assert!(!body.contains("data:image/"));
        assert!(!body.contains("\"type\":\"input_image\""));
        assert!(body.contains("A solid blue rectangle."));
        assert!(body.contains("image_reference"));
        assert!(body.contains("PRIVATE_MAIN_CONTEXT_DO_NOT_FORWARD"));
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
async fn replay_localizes_uncached_images_without_inference_or_rewriting_old_records()
-> anyhow::Result<()> {
    let server = start_mock_server().await;
    let mut initial_builder = test_codex().with_config(|config| {
        let _ = config.features.disable(Feature::ImageReferenceContext);
    });
    let initial = initial_builder.build(&server).await?;
    let path = initial.workspace_path("historical.png");
    ImageBuffer::from_pixel(2, 3, Rgba([0u8, 0, 255, 255])).save(&path)?;
    mount_sse_sequence(
        &server,
        vec![sse(vec![
            ev_response_created("initial"),
            ev_assistant_message("initial-answer", "Blue."),
            ev_completed("initial"),
        ])],
    )
    .await;
    initial
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::LocalImage {
            path,
            detail: None,
        }]))
        .await?;
    wait_for_event_with_timeout(
        &initial.codex,
        |e| matches!(e, EventMsg::TurnComplete(_)),
        Duration::from_secs(30),
    )
    .await;
    let rollout = initial.codex.rollout_path().context("rollout path")?;
    initial.codex.shutdown_and_wait().await?;
    let old_records = std::fs::read_to_string(&rollout)?;
    assert!(old_records.contains("data:image/"));

    let resumed_server = start_mock_server().await;
    let mock = mount_sse_sequence(
        &resumed_server,
        vec![sse(vec![
            ev_response_created("resumed"),
            ev_assistant_message("resumed-answer", "No cached visual description."),
            ev_completed("resumed"),
        ])],
    )
    .await;
    let mut resumed_builder = test_codex().with_config(|config| {
        let _ = config.features.enable(Feature::ImageReferenceContext);
    });
    let resumed = resumed_builder
        .resume(&resumed_server, initial.home.clone(), rollout.clone())
        .await?;
    resumed
        .submit_turn_with_permission_profile("continue", PermissionProfile::Disabled)
        .await?;
    let requests = mock.requests();
    assert_eq!(
        requests.len(),
        1,
        "replay must not start an isolated visual request"
    );
    let main = requests[0].body_json().to_string();
    assert!(!main.contains("data:image/"));
    assert!(main.contains("image_reference"));
    assert!(main.contains("unavailable"));
    resumed.codex.flush_rollout().await?;
    assert!(std::fs::read_to_string(rollout)?.starts_with(&old_records));
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_inline_image_does_not_leak_through_user_ui_events() -> anyhow::Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_config(|config| {
        let _ = config.features.enable(Feature::ImageReferenceContext);
    });
    let test = builder.build(&server).await?;
    let mock = mount_sse_sequence(
        &server,
        vec![sse(vec![
            ev_response_created("main"),
            ev_assistant_message("answer", "Image unavailable."),
            ev_completed("main"),
        ])],
    )
    .await;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Image {
            image: ImageReference::Inline {
                image_url: "data:image/png;base64,INVALID_PAYLOAD!".to_string(),
            },
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
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].body_json().to_string().contains("data:image/"));
    test.codex.flush_rollout().await?;
    let rollout = test.codex.rollout_path().context("rollout path")?;
    let history = std::fs::read_to_string(rollout)?;
    assert!(!history.contains("data:image/"));
    assert!(!history.contains("INVALID_PAYLOAD"));
    Ok(())
}
