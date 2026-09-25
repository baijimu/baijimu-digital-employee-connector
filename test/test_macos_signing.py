import base64
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/release"))
import macos_signing


class MacSigningTest(unittest.TestCase):
    def exercise(self, fail_at=None):
        calls = []
        registered = False
        def execute(arguments, **kwargs):
            nonlocal registered
            calls.append(arguments)
            if arguments[:5] == ["security", "list-keychains", "-d", "user", "-s"]:
                registered = len(arguments) == 8
            if arguments[0] == "codesign":
                self.assertTrue(registered, "signing private key is not registered")
            failed = arguments[:2] == fail_at
            return subprocess.CompletedProcess(arguments, 1 if failed else 0)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            environment = {"APPLE_CERTIFICATE": base64.b64encode(b"fixture").decode(),
                           "APPLE_CERTIFICATE_PASSWORD": "fixture-password"}
            with patch.dict(macos_signing.os.environ, environment), \
                 patch.object(macos_signing.subprocess, "check_output", return_value='"/original/keychain with spaces"\n"/original/login"\n'), \
                 patch.object(macos_signing.subprocess, "run", side_effect=execute):
                if fail_at:
                    with self.assertRaises(RuntimeError) as error:
                        macos_signing.sign(root / "binary", root, "fixture-identity")
                    self.assertNotIn("fixture-password", str(error.exception))
                else:
                    macos_signing.sign(root / "binary", root, "fixture-identity")
            self.assertFalse((root / "certificate.p12").exists())
            self.assertEqual(calls[-2], ["security", "list-keychains", "-d", "user", "-s",
                                        "/original/keychain with spaces", "/original/login"])
            self.assertEqual(calls[-1], ["security", "delete-keychain", str(root / "signing.keychain-db")])
        return calls

    def test_signs_after_registration_and_restores_original_search_list(self):
        calls = self.exercise()
        self.assertTrue(any(call[:3] == ["codesign", "--verify", "--strict"] for call in calls))

    def test_import_signing_and_verification_errors_still_restore_and_delete(self):
        for operation in (["security", "import"], ["codesign", "--force"], ["codesign", "--verify"]):
            with self.subTest(operation=operation):
                self.exercise(operation)
