"""Validate all platforms before any external publication side effects."""
import hashlib
import json
import zipfile
from pathlib import Path
from html.parser import HTMLParser
from pathlib import PurePosixPath

TARGETS = {"macos": "universal", "windows": "x86_64", "linux": "x86_64"}


class UiAssets(HTMLParser):
    def __init__(self):
        super().__init__()
        self.paths = []

    def handle_starttag(self, tag, attributes):
        attributes = dict(attributes)
        key = 'href' if tag == 'link' else 'src' if tag in ('script', 'img') else None
        if key and key in attributes:
            self.paths.append(attributes[key])


def validate_ui(package, manifest):
    if 'ui' not in manifest:
        return
    entry = PurePosixPath(manifest['ui']['entry'])
    if entry.is_absolute() or '..' in entry.parts or len(entry.parts) < 2:
        raise ValueError('Invalid embedded UI entry')
    parser = UiAssets()
    parser.feed(package.read(str(entry)).decode('utf-8'))
    for value in parser.paths:
        asset = PurePosixPath(value)
        if asset.is_absolute() or '..' in asset.parts or ':' in value:
            raise ValueError('Embedded UI asset must stay inside its UI directory')
        if package.getinfo(str(entry.parent / asset)).file_size == 0:
            raise ValueError('Empty embedded UI asset')


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
            validate_ui(package, manifest)
            folder = "macos" if platform == "macos" else platform + "-" + architecture
            binary = "bin/" + folder + "/" + manifest["runtime"]["command"]
            if platform == "windows":
                binary += ".exe"
            if package.getinfo(binary).file_size == 0:
                raise ValueError("Empty runtime binary: " + platform)
        rows.append(row)
    return rows
