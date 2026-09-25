import importlib.util
import hashlib
import json
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
import zipfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools/release"))
from artifact_contract import TARGETS, validate_artifacts
from preflight import PUBLICATION, SIGNING, validate

spec = importlib.util.spec_from_file_location("recovery", ROOT / "tools/release/recover-artifacts.py")
recovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recovery)
spec = importlib.util.spec_from_file_location("native", ROOT / "tools/release/publish-native.py")
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class ReleaseArtifactsTest(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.directory = Path(temp.name)
        self.manifest = json.loads((ROOT / "connector.json").read_text(encoding="utf-8"))
        self.rows = []
        for platform, architecture in TARGETS.items():
            name = f"{self.manifest['source']['repo'].split('/')[-1]}-{self.manifest['version']}-{platform}-{architecture}.zip"
            folder = "macos" if platform == "macos" else platform + "-" + architecture
            binary = f"bin/{folder}/{self.manifest['runtime']['command']}" + (".exe" if platform == "windows" else "")
            with zipfile.ZipFile(self.directory / name, "w") as archive:
                archive.writestr("connector.json", json.dumps(self.manifest))
                archive.writestr(binary, b"fixture executable")
            digest = hashlib.sha256((self.directory / name).read_bytes()).hexdigest()
            row = {"name": name, "platform": platform, "arch": architecture, "checksum": "sha256:" + digest}
            self.rows.append(row)
            self.write(platform + ".json", row)
            (self.directory / (name + ".sha256")).write_text(digest + "  " + name + "\n")
            self.write("provenance-" + platform + ".json", {
                **row, "schemaVersion": 1, "repository": self.manifest["source"]["repo"],
                "revision": self.manifest["source"]["revision"], "commit": "fixture-sha",
                "publish": True, "runId": "123", "runAttempt": "1",
            })

    def write(self, name, value):
        (self.directory / name).write_text(json.dumps(value), encoding="utf-8")

    def check(self):
        return validate_artifacts(self.directory, self.manifest, "fixture-sha", "123")

    def test_complete_original_build_is_recoverable(self):
        self.assertEqual(self.check(), self.rows)

    def test_unsigned_other_commit_or_other_run_cannot_be_published(self):
        path = self.directory / "provenance-windows.json"
        original = json.loads(path.read_text())
        for key, value in [("publish", False), ("commit", "other"), ("runId", "456"),
                           ("revision", "v9.9.9"), ("repository", "other/repo")]:
            with self.subTest(key=key):
                self.write(path.name, {**original, key: value})
                with self.assertRaises(ValueError):
                    self.check()

    def test_artifact_corruption_and_sidecar_mismatch_stop_before_upload(self):
        path = self.directory / self.rows[0]["name"]
        original = path.read_bytes()
        path.write_bytes(original + b"corrupted")
        with self.assertRaisesRegex(ValueError, "digest"):
            self.check()
        path.write_bytes(original)
        (self.directory / (path.name + ".sha256")).write_text("wrong")
        with self.assertRaisesRegex(ValueError, "sidecar"):
            self.check()

    def test_missing_platform_and_path_traversal_are_rejected(self):
        row = {**self.rows[0], "name": "../other.zip"}
        self.write("macos.json", row)
        with self.assertRaisesRegex(ValueError, "filename"):
            self.check()
        self.write("macos.json", self.rows[0])
        (self.directory / "windows.json").unlink()
        with self.assertRaises(FileNotFoundError):
            self.check()

    def test_recovery_rejects_unrelated_or_running_workflows(self):
        run = {"id": 123, "repository": {"full_name": "owner/repo"},
               "path": ".github/workflows/release.yml", "event": "workflow_dispatch", "status": "completed"}
        recovery.validate_run(run, "owner/repo", "123")
        for key, value in [("id", 456), ("path", ".github/workflows/ci.yml"),
                           ("event", "pull_request"), ("status", "in_progress"),
                           ("repository", {"full_name": "other/repo"})]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                recovery.validate_run({**run, key: value}, "owner/repo", "123")

    def test_preflight_separates_verification_signing_and_recovery(self):
        validate({"PUBLISH": "false"})
        configuration = {name: "present" for name in PUBLICATION + SIGNING}
        validate({**configuration, "PUBLISH": "true"})
        with self.assertRaisesRegex(ValueError, "APPLE_CERTIFICATE"):
            validate({**{name: "present" for name in PUBLICATION}, "PUBLISH": "true"})
        validate({**{name: "present" for name in PUBLICATION}, "PUBLISH": "true", "RECOVERY_RUN_ID": "123"})
        for value in ("abc", "0", "-1"):
            with self.assertRaises(ValueError):
                validate({**configuration, "PUBLISH": "true", "RECOVERY_RUN_ID": value})
        with self.assertRaises(ValueError):
            validate({"PUBLISH": "false", "RECOVERY_RUN_ID": "123"})

    def test_github_auth_or_network_failure_is_not_release_absence(self):
        for response in ["HTTP/2.0 403 Forbidden\n\n{}", "HTTP/2.0 500 Server Error\n\n{}", ""]:
            with patch.object(native.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, response, "private diagnostic")):
                with self.assertRaisesRegex(RuntimeError, "absence was not confirmed"):
                    native.github_release("owner/repo", "v1.0.0")
        response = subprocess.CompletedProcess([], 1, "HTTP/2.0 404 Not Found\r\n\r\n{}", "")
        with patch.object(native.subprocess, "run", return_value=response):
            self.assertIsNone(native.github_release("owner/repo", "v1.0.0"))
        response = subprocess.CompletedProcess([], 0, 'HTTP/2.0 200 OK\n\n{"assets": []}', "")
        with patch.object(native.subprocess, "run", return_value=response):
            self.assertEqual(native.github_release("owner/repo", "v1.0.0"), {"assets": []})


if __name__ == "__main__":
    unittest.main()
