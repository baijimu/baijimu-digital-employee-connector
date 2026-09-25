"""Real private app-server smoke check; no login or model turn is performed."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--connector", required=True)
    parser.add_argument("--codex", required=True)
    args = parser.parse_args()
    binary = str(Path(args.connector).resolve(strict=True))
    codex = str(Path(args.codex).resolve(strict=True))
    with tempfile.TemporaryDirectory(prefix="employee-smoke-") as temporary:
        root = Path(temporary)
        state = root / "state"
        cwd = root / "project"
        cwd.mkdir()
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        environment = {k: v for k, v in os.environ.items()
                       if not k.upper().startswith(("BAIJIMU_", "DIGITAL_EMPLOYEE_"))}
        environment.update(BAIJIMU_LOCAL_APP_DATA_DIR=str(state), DIGITAL_EMPLOYEE_CODEX_BINARY=codex)
        token = None
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def request(path, data=None, authorized=True, workspace=True):
            headers = {}
            if authorized and token:
                headers["Authorization"] = "Bearer " + token
            if workspace:
                # Local fixture context, never sent to the platform.
                headers["x-baijimu-workspace-id"] = "1"
            payload = None if data is None else json.dumps(data).encode()
            req = urllib.request.Request(f"http://127.0.0.1:{port}" + path, data=payload, headers=headers)
            try:
                with opener.open(req, timeout=30) as response:
                    return response.status, json.load(response)
            except urllib.error.HTTPError as error:
                return error.code, json.load(error)

        with (root / "connector.log").open("wb") as log:
            process = subprocess.Popen([binary, "start", "--port", str(port)], env=environment,
                                       stdout=log, stderr=log, stdin=subprocess.DEVNULL)
            child_pid = None
            try:
                deadline = time.monotonic() + 10
                while True:
                    if process.poll() is not None:
                        raise RuntimeError("Connector exited during startup")
                    try:
                        status, health = request("/healthz")
                        if status == 200:
                            break
                    except (OSError, urllib.error.URLError):
                        pass
                    if time.monotonic() > deadline:
                        raise RuntimeError("Connector startup timeout")
                    time.sleep(0.1)
                assert health["status"]["connector"]["pid"] == process.pid
                token = (state / "management-token").read_text().strip()
                assert request("/invoke/status", {}, authorized=False)[0] == 401
                assert request("/invoke/status", {}, workspace=False)[0] == 400
                status, account = request("/invoke/request", {"method": "account/read", "params": {"refreshToken": False}})
                assert status == 200, account
                assert account["data"]["result"]["account"] is None, "Fresh employee inherited a login"
                status, runtime = request("/invoke/status", {})
                app = runtime["data"]["appServer"]
                child_pid = app["pid"]
                assert status == 200 and app["running"] and app["initialized"] and not app["shared"]
                assert Path(app["codexHome"]).resolve() == (state / "runtime/codex").resolve()
                status, created = request("/invoke/startThread", {"cwd": str(cwd),
                    "params": {"approvalPolicy": "never", "sandbox": "read-only"}})
                assert status == 200, created
                task = created["data"]["result"]["thread"]["id"]
                status, read = request("/invoke/readThread", {"threadId": task})
                assert status == 200 and read["data"]["result"]["thread"]["id"] == task, read
                print("PASS: HTTP authorization, workspace context, private app-server, isolated account, thread start/read")
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            if child_pid and os.name == "posix":
                deadline = time.monotonic() + 10
                while True:
                    try:
                        os.kill(child_pid, 0)
                    except ProcessLookupError:
                        break
                    if time.monotonic() > deadline:
                        raise RuntimeError("Private app-server survived Connector shutdown")
                    time.sleep(0.1)
                print("PASS: private app-server exits with Connector")


if __name__ == "__main__":
    main()
