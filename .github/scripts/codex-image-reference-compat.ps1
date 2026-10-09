# Compatibility launcher for the original 0.0.0 Windows archive.
# Keep this script beside bin/ in the extracted package directory.
$binary = Join-Path $PSScriptRoot 'bin\codex.exe'
if (-not (Test-Path -LiteralPath $binary)) {
    throw "Missing custom CLI: $binary. Keep the extracted package directory intact."
}
$provider = 'model_providers.image-reference-compat={name="OpenAI",wire_api="responses",requires_openai_auth=true,supports_websockets=true,http_headers={version="0.160.1-image-reference.1"}}'
$forwarded = @($args)
if ($forwarded.Count -gt 0 -and $forwarded[0] -eq 'exec') {
    $remaining = @($forwarded | Select-Object -Skip 1)
    & $binary --enable image_reference_context exec -c 'model_provider="image-reference-compat"' -c $provider @remaining
} else {
    & $binary --enable image_reference_context -c 'model_provider="image-reference-compat"' -c $provider @forwarded
}
exit $LASTEXITCODE
