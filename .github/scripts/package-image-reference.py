"""Package native CLI companions without shipping build caches or local data."""
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tarfile
import zipfile

target = sys.argv[1]
root = Path(__file__).resolve().parents[2]
release = root / "codex-rs" / "target" / target / "release"
name = "codex-image-reference-" + target
stage = root / "dist" / "staging" / name
stage.mkdir(parents=True, exist_ok=True)
(stage / "codex-resources").mkdir(exist_ok=True)
(stage / "codex-path").mkdir(exist_ok=True)
suffix = ".exe" if "windows" in target else ""
for binary in ["codex", "codex-app-server", "codex-code-mode-host", "codex-responses-api-proxy"]:
    source = release / (binary + suffix)
    if not source.is_file():
        raise SystemExit("Missing built companion: " + str(source))
    (stage / "bin").mkdir(exist_ok=True)
    shutil.copy2(source, stage / "bin" / source.name)
if "linux" in target:
    shutil.copy2(release / "bwrap", stage / "codex-resources" / "bwrap")
if "windows" in target:
    for binary in ["codex-command-runner.exe", "codex-windows-sandbox-setup.exe"]:
        shutil.copy2(release / binary, stage / "codex-resources" / binary)
(stage / "codex-package.json").write_text(json.dumps({
    "layoutVersion": 1,
    "version": "0.0.0",
    "target": target,
    "variant": "codex",
    "entrypoint": "bin/codex" + suffix,
    "resourcesDir": "codex-resources",
    "pathDir": "codex-path",
}, indent=2) + "\n", encoding="utf-8")
for source in [root / "LICENSE", root / "README-image-references.md"]:
    shutil.copy2(source, stage / source.name)
if (root / "NOTICE").is_file():
    shutil.copy2(root / "NOTICE", stage / "NOTICE")
manifest = {p.relative_to(stage).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in stage.rglob("*") if p.is_file()}
(stage / "SHA256SUMS.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
dist = root / "dist"
if "windows" in target:
    archive = dist / (name + ".zip")
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as stream:
        for p in stage.rglob("*"):
            stream.write(p, p.relative_to(stage.parent).as_posix())
else:
    archive = dist / (name + ".tar.gz")
    with tarfile.open(archive, "w:gz") as stream:
        stream.add(stage, arcname=name)
checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
(dist / (archive.name + ".sha256")).write_text(checksum + "  " + archive.name + "\n", encoding="utf-8")
print(archive.name + " " + checksum)
