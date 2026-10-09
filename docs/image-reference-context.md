# Experimental image reference context

`image_reference_context` is an opt-in, under-development feature for image-heavy conversations. Enable it with `codex --enable image_reference_context` or `[features] image_reference_context = true` in the configuration.

Instead of inserting a structured image into main-thread history, Codex saves the original bytes in `$CODEX_HOME/image-artifacts/<sha256>.<extension>` and performs a separate, tool-free visual request using the selected model. The analysis uses the existing resized high-detail preparation, while the original bytes remain on disk. The main conversation receives a local reference, dimensions, hash, analysis status, and a factual description. `view_image` also returns this text-only result, including in code mode. The default disabled behavior remains unchanged.

The isolated request contains the image and a fixed description instruction, without the conversation history or tools. Image-derived text is explicitly marked as untrusted. This limits the request's scope; it does not guarantee that a generated description is accurate or immune to instructions in an image.

Successful descriptions are cached by image content and model. The cache limits descriptions to 2,048 Unicode characters. A cache miss incurs one additional inference request, with a 90-second timeout. Usage is recorded, but the isolated request is excluded from inference traces to avoid recording its image content there. Description-request failures retain the stored reference with an unavailable status and never fall back to inline image history.

The implementation processes user attachments, structured image-bearing tool outputs, and `view_image`. It does not scan arbitrary text for Base64 or change generated-image protocol events. Existing rollout files are left intact; reconstructed image history is prepared when resumed using cached descriptions only. Replay never makes a new visual request. An uncached historical image receives an unavailable reference until explicitly analyzed with `view_image`. Previously uploaded file IDs without local bytes cannot be described. Ephemeral sessions fail closed instead of creating persistent image artifacts.

This mode trades visual fidelity for smaller main-thread history. Persisted artifacts currently have no automatic retention policy. References identify files on the Codex host; they do not provide remote-environment file access. These lifecycle and remote-environment semantics require further design before promoting the feature out of development.

## Focused validation

From `codex-rs`, run:

```sh
cargo test --locked -p codex-utils-image artifacts::tests --lib
cargo test --locked -p codex-core --test image_reference_context
```

Storage tests cover original-byte preservation, reference serialization, Unicode description bounds, model-separated caches, corrupted artifacts, and oversized description caches. Integration tests use a local mock Responses server and cover the disabled default, tool-free isolated requests without main-thread text, cache reuse across local and inline attachments, text-only main requests and rollouts, failed analysis, invalid-image UI events, `view_image`, and replay without new inference or historical record edits. They do not require account credentials or paid inference.
