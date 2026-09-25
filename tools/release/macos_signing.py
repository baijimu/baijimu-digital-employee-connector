"""Sign with a temporary keychain registered for Security framework lookup."""
import base64
import os
import shlex
import subprocess


def run(arguments):
    result = subprocess.run(arguments, stdout=subprocess.DEVNULL)
    if result.returncode:
        # Arguments can contain credentials; never include them in exceptions.
        raise RuntimeError("macOS signing operation failed: " + arguments[0])


def sign(binary, directory, identity):
    certificate = directory / "certificate.p12"
    certificate.write_bytes(base64.b64decode(os.environ["APPLE_CERTIFICATE"]))
    keychain = directory / "signing.keychain-db"
    password = os.urandom(24).hex()
    original = shlex.split(subprocess.check_output(
        ["security", "list-keychains", "-d", "user"], text=True))
    created = False
    try:
        run(["security", "create-keychain", "-p", password, str(keychain)])
        created = True
        run(["security", "set-keychain-settings", "-lut", "21600", str(keychain)])
        run(["security", "unlock-keychain", "-p", password, str(keychain)])
        run(["security", "import", str(certificate), "-k", str(keychain),
             "-P", os.environ["APPLE_CERTIFICATE_PASSWORD"], "-T", "/usr/bin/codesign"])
        # --keychain alone does not make the imported private key discoverable.
        run(["security", "list-keychains", "-d", "user", "-s", str(keychain), *original])
        run(["security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:",
             "-s", "-k", password, str(keychain)])
        run(["codesign", "--force", "--timestamp", "--options", "runtime", "--keychain",
             str(keychain), "--sign", identity, str(binary)])
        run(["codesign", "--verify", "--strict", str(binary)])
    finally:
        try:
            if created:
                try:
                    run(["security", "list-keychains", "-d", "user", "-s", *original])
                finally:
                    run(["security", "delete-keychain", str(keychain)])
        finally:
            certificate.unlink(missing_ok=True)
