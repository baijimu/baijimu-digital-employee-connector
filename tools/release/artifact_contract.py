"""Validate all platforms before any external publication side effects."""
import hashlib
import json
import zipfile
from pathlib import Path

TARGETS = {"macos": "universal", "windows": "x86_64", "linux": "x86_64"}


def validate_artifacts(directory, manifest, commit, run_id):
    directory = Path(directory)
    rows = []
    for platform, architecture in TARGETS.items():
        row = json.loads((directory / (platform + ".json")).read_text(encoding="utf-8"))
        name = f"{manifest['source']['repo'].split('/')[-1]}-{manifest['version']}-{platform}-{architecture}.zip"
        if row.get("name") != name or row.get("platform") != platform or row.get("arch") != architecture:
            raise ValueError("Artifact target or filename mismatch: " + platform)
        archive = directory / name
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if row.get("checksum") != "sha256:" + digest:
            raise ValueError("Artifact digest mismatch: " + platform)
        if (directory / (name + ".sha256")).read_text(encoding="utf-8") != digest + "  " + name + "\n":
            raise ValueError("Artifact checksum sidecar mismatch: " + platform)
        provenance = json.loads((directory / ("provenance-" + platform + ".json")).read_text(encoding="utf-8"))
        expected = {
            "schemaVersion": 1, "repository": manifest["source"]["repo"],
            "revision": manifest["source"]["revision"], "commit": commit,
            "publish": True, "runId": str(run_id), **row,
        }
        if any(provenance.get(key) != value for key, value in expected.items()):
            raise ValueError("Artifact must come from a publish build of this exact source: " + platform)
        if not str(provenance.get("runAttempt", "")).isdigit():
            raise ValueError("Artifact run attempt missing: " + platform)
        with zipfile.ZipFile(archive) as package:
            if len(package.namelist()) != len(set(package.namelist())):
                raise ValueError("Duplicate archive entries: " + platform)
            if json.loads(package.read("connector.json")) != manifest:
                raise ValueError("Packaged manifest mismatch: " + platform)
            folder = "macos" if platform == "macos" else platform + "-" + architecture
            binary = "bin/" + folder + "/" + manifest["runtime"]["command"]
            if platform == "windows":
                binary += ".exe"
            if package.getinfo(binary).file_size == 0:
                raise ValueError("Empty runtime binary: " + platform)
        rows.append(row)
    return rows
