# Codex: local image references and isolated descriptions

Experimental, unofficial fork of [openai/codex](https://github.com/openai/codex), initially based on `a645742`.

## Purpose

Large screenshots should not be replayed as inline Base64 in every main-model request. With this feature enabled, the main conversation contains a local disk reference and a factual text description. Original image bytes are stored separately on disk.

## Enable

Run the custom binary with `--enable image_reference_context`, or enable `image_reference_context = true` inside the `[features]` table in its Codex configuration. The feature is opt-in; upstream behavior remains unchanged when disabled.

The build archives contain `bin/codex`, `bin/codex-app-server`, `bin/codex-code-mode-host`, and `bin/codex-responses-api-proxy` (with `.exe` suffixes on Windows). Linux archives also contain the sandbox `resources/bwrap`.

This is a custom CLI/app-server build. Installing the CLI alone does not change the binary bundled with Codex Desktop.

## Image lifecycle

1. Read and validate the image using the existing tool/environment read permissions.
2. Preserve its original bytes in `$CODEX_HOME/image-artifacts/<sha256>.<extension>`.
3. On a description-cache miss, perform a separate, tool-free visual request using the selected vision-capable model, low reasoning effort, and only the image plus a fixed description instruction.
4. Cache the bounded description by image content and model.
5. Put ordinary text containing the local path, hash, dimensions, description, and analysis status into the main history. The original picture is not embedded in that history.

Descriptions are limited to 2,048 Unicode characters. Images are limited to 64 MiB. Successful duplicate-image analysis is deduplicated, including simultaneous callers. The auxiliary visual request is bounded by a 90-second timeout.

Images still leave the machine for the initial visual-analysis request, and that request can consume vision/model usage. The benefit is eliminating repeated image bytes from the main conversation; this is not an offline vision implementation.

## `view_image` in code mode

In this mode, the output is an object containing `image_reference`, `description`, and `analysis_status`. It is not an `image_url` object. Forward the description as text instead of passing the result to an image emitter. The tool schema advertises this difference when the feature is enabled.

## Failure behavior and scope

- Analysis failures retain the valid local disk reference with `analysis_status = unavailable`; no visual facts are invented and there is no inline-image fallback.
- User-supplied local/inline images, structured image-bearing function/MCP/code-mode outputs, and `view_image` are processed before main-history insertion.
- User-message UI events use cached local image paths rather than inline data URLs.
- Existing rollouts are never edited. When an old rollout is resumed with the feature enabled, its reconstructed in-memory image history is processed while original records remain intact.
- Pre-existing uploaded `file_id` images without available local bytes receive an explicit unavailable-reference message; use their original local image with `view_image` for a description.
- Ephemeral sessions do not enable durable image artifacts. Image processing fails closed with explanatory text rather than persisting image content unexpectedly.
- Text descriptions may lose small visual details. The original image remains available for local inspection or a fresh analysis after changing the image/model.

## Validation

Focused integration tests use a local mock Responses server. They verify that only the isolated visual request contains image data, that subsequent main requests contain references/descriptions, that repeat images reuse descriptions, that rollouts do not embed image data, and that failed analysis never falls back to inline images.

Run the focused `image_reference_context` integration test target in `codex-core`, and the `artifacts::tests` library tests in `codex-utils-image` using the repository Rust toolchain. These tests do not send real images to a paid model endpoint.

## CI artifacts

The dedicated workflow tests the feature before building native x86_64 and ARM64 archives for Linux, macOS, and Windows. Every archive contains checksums and preserves the upstream license. Successful builds publish a GitHub Release.

Linux GNU builds use Ubuntu 24.04 runners. macOS builds use macOS 15 runners. Windows builds use the Microsoft SDK/MSVC environment. These are unsigned community build artifacts.

Upstream production workflows are preserved under `.github/upstream-workflows` so this fork does not attempt upstream signing, internal runners, package publishing, or unrelated automation.
