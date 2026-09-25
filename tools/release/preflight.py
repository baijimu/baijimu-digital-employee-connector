"""Check presence only; never print or export secret values."""
import os

PUBLICATION = (
    "OSS_ACCESS_KEY_ID", "OSS_ACCESS_KEY_SECRET", "OSS_BUCKET", "OSS_ENDPOINT",
    "OSS_REGION", "OSS_PUBLIC_BASE", "LOCAL_APP_MARKET_PUBLISH_TOKEN",
    "LOCAL_APP_OWNER_WORKSPACE_ID",
)
SIGNING = (
    "APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY",
    "SSL_COM_USERNAME", "SSL_COM_PASSWORD", "SSL_COM_CREDENTIAL_ID", "SSL_COM_TOTP_SECRET",
)


def validate(environ):
    recovery = environ.get("RECOVERY_RUN_ID", "")
    if recovery and (not recovery.isdigit() or int(recovery) <= 0 or environ.get("PUBLISH") != "true"):
        raise ValueError("Recovery requires publish=true and a positive workflow run ID")
    if environ.get("PUBLISH") != "true":
        return
    required = PUBLICATION if recovery else PUBLICATION + SIGNING
    missing = [name for name in required if not environ.get(name, "").strip()]
    if missing:
        raise ValueError("Missing release configuration: " + ", ".join(missing))


if __name__ == "__main__":
    validate(os.environ)
    print("Release configuration preflight passed")
