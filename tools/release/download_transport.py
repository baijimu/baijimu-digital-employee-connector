"""Select the CI transport without rewriting immutable public artifact URLs."""
import os
from urllib.parse import urlsplit, urlunsplit


def transport_url(url):
    public = os.environ.get("OSS_PUBLIC_BASE", "").rstrip("/")
    transport = os.environ.get("OSS_DOWNLOAD_BASE", "").rstrip("/")
    if not transport:
        return url
    for base in (public, transport):
        parsed = urlsplit(base)
        if (parsed.scheme != "https" or not parsed.hostname or parsed.username
                or parsed.password or parsed.query or parsed.fragment):
            raise ValueError("Download bases must be credential-free HTTPS URLs")
    # Only the configured object's origin/path prefix may be translated.
    # External tool sources and signed URLs keep their original authority.
    parsed = urlsplit(url)
    if parsed.query or parsed.fragment or parsed.username or parsed.password:
        return url
    if url.startswith(public + "/"):
        return transport + url[len(public):]
    return url


def safe_url(url):
    parsed = urlsplit(url)
    return urlunsplit((parsed.scheme, parsed.hostname or "", parsed.path, "", ""))
