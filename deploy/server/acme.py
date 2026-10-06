"""Obtain/renew public IP or DNS certificates with the official Certbot image.

Only TLS files are shared with EVK; the ACME account stays in its own volume.
The EVK supervisor validates and reloads nginx, without Docker socket access.
"""

import hashlib
import ipaddress
import os
from pathlib import Path
import re
import signal
import ssl
import subprocess
import sys
import tempfile
import threading


TLS = Path("/run/evk-tls")
LINEAGE = Path("/etc/letsencrypt/live/evk")
PRODUCTION = "https://acme-v02.api.letsencrypt.org/directory"


def certbot_command(env):
    host = env.get("EVK_HOST", "")
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        address = None
        label = r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?"
        pattern = rf"(?:{label}\.)+[a-z](?:[a-z0-9-]{{0,61}}[a-z0-9])?"
        if len(host) > 253 or not re.fullmatch(pattern, host):
            raise ValueError("EVK_HOST must be a public IP or lowercase DNS hostname")
    if address and (not address.is_global or address.is_multicast or "%" in host):
        raise ValueError("Let's Encrypt requires a public IP address")
    if env.get("EVK_ACME_AGREE_TOS") != "yes":
        raise ValueError(
            "Read the Let's Encrypt subscriber agreement and set EVK_ACME_AGREE_TOS=yes"
        )
    if env.get("EVK_PREVIEW_DOMAIN"):
        raise ValueError(
            "Automatic certificates cover the app only; leave EVK_PREVIEW_DOMAIN "
            "empty or use externally managed wildcard TLS"
        )
    command = [
        "certbot", "certonly", "--non-interactive", "--agree-tos",
        "--standalone", "--preferred-challenges", "http",
        "--server", PRODUCTION, "--cert-name", "evk",
        "--preferred-profile", "shortlived", "--keep-until-expiring",
        "--renew-with-new-domains",
        "--deploy-hook", "python /opt/evk-acme/acme.py publish",
    ]
    command += ["--ip-address" if address else "--domains", host]
    email = env.get("EVK_ACME_EMAIL")
    command += ["--email", email] if email else ["--register-unsafely-without-email"]
    return command


def publish(lineage=LINEAGE, destination=TLS):
    certificate = (lineage / "fullchain.pem").read_bytes()
    key = (lineage / "privkey.pem").read_bytes()
    # Validate a complete pair before exposing it to nginx.
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(lineage / "fullchain.pem", lineage / "privkey.pem")
    destination.mkdir(mode=0o750, parents=True, exist_ok=True)
    os.chown(destination, 10001, 10001)
    os.chmod(destination, 0o750)
    for name in ("fullchain.pem", "privkey.pem"):
        path = destination / name
        if path.is_symlink():
            if os.readlink(path) != f"current/{name}":
                raise ValueError(f"Refusing to replace an external TLS symlink: {path}")
        elif path.exists():
            raise ValueError(
                "Existing manually managed TLS files; move them aside "
                "before enabling automatic certificates"
            )
    fingerprint = hashlib.sha256(certificate + key).hexdigest()
    generation = destination / fingerprint
    if not generation.exists():
        temporary = Path(tempfile.mkdtemp(prefix=".new-", dir=destination))
        os.chown(temporary, 10001, 10001)
        os.chmod(temporary, 0o750)
        for name, content in (("fullchain.pem", certificate), ("privkey.pem", key)):
            file = temporary / name
            file.write_bytes(content)
            os.chown(file, 10001, 10001)
            os.chmod(file, 0o640)
        temporary.rename(generation)
    pointer = destination / ".current-new"
    pointer.unlink(missing_ok=True)
    pointer.symlink_to(generation.name)
    pointer.replace(destination / "current")
    for name in ("fullchain.pem", "privkey.pem"):
        path = destination / name
        if not path.is_symlink():
            path.symlink_to(f"current/{name}")


def health(destination=TLS):
    from cryptography import x509
    from datetime import datetime, timedelta, timezone
    certificate = x509.load_pem_x509_certificate(
        (destination / "fullchain.pem").read_bytes()
    )
    now = datetime.now(timezone.utc)
    expiry_warning = certificate.not_valid_after_utc - timedelta(hours=6)
    if not certificate.not_valid_before_utc <= now < expiry_warning:
        raise ValueError(
            "TLS certificate is missing, not yet valid or expires within six hours"
        )
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(destination / "fullchain.pem", destination / "privkey.pem")


def run():
    command = certbot_command(os.environ)
    stopped = threading.Event()
    child = None

    def stop(_signal, _frame):
        stopped.set()
        if child is not None and child.poll() is None:
            child.terminate()

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    while not stopped.is_set():
        child = subprocess.Popen(command)
        result = child.wait()
        if stopped.is_set():
            break
        try:
            # Also repair export after a failed deploy hook or a lost TLS bind
            # directory, even when Certbot says renewal is not yet due.
            publish()
        except Exception as error:
            print(f"TLS export failed: {error}", file=sys.stderr, flush=True)
            result = 1
        delay = 3600 if result == 0 else 900
        status = "succeeded" if result == 0 else "failed"
        print(f"Certificate check {status}; retry in {delay}s", flush=True)
        stopped.wait(delay)


if __name__ == "__main__":
    try:
        if sys.argv[1:] == ["run"]:
            run()
        elif sys.argv[1:] == ["publish"]:
            publish(Path(os.environ["RENEWED_LINEAGE"]))
        elif sys.argv[1:] == ["health"]:
            health()
        else:
            raise ValueError("Usage: acme.py run|publish|health")
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
