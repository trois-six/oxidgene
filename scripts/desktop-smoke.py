"""Smoke-test a packaged desktop download before it is released.

    python3 scripts/desktop-smoke.py <archive>

The release workflow runs it on each platform's download, as packaged: the
Linux .tar.gz, the macOS .dmg, the Windows .zip (docs/development.md §6). It
unpacks the executable and checks, without opening a window:

- that `--version` names the workspace's version;
- on macOS, that the executable is universal (Apple silicon and Intel);
- an MCP session over standard input and output (`oxidgene-desktop mcp`,
  docs/mcp.md §4): initialize, then a `list_trees` call answered with the
  empty list of a fresh database, every line of standard output a protocol
  message, and a clean exit once the input closes.

The subcommand opens only a database the application created, so the script
puts an empty one where the platform keeps it: under a temporary
`XDG_DATA_HOME` on Linux and a temporary `HOME` on macOS. Windows resolves
its application data folder without reading the environment, so there the
database goes to the real `%APPDATA%\\oxidgene`, on a disposable runner: the
script refuses to run when one is already there, and removes it afterwards.

Standard library only, so it runs on any runner's Python.
"""

import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TIMEOUT = 60


def fail(message: str) -> None:
    print(f"desktop smoke: {message}", file=sys.stderr)
    sys.exit(1)


def workspace_version() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^\[workspace\.package\]\n(?:.*\n)*?version = "([^"]+)"', text, re.MULTILINE)
    if not match:
        fail("no workspace version in Cargo.toml")
    return match.group(1)


def unpack(archive: Path, work: Path) -> Path:
    """The executable of `archive`, unpacked under `work`."""
    name = archive.name
    if name.endswith(".tar.gz"):
        with tarfile.open(archive) as tar:
            tar.extractall(work, filter="data")
        return work / "oxidgene-desktop"
    if name.endswith(".zip"):
        with zipfile.ZipFile(archive) as zip_file:
            zip_file.extractall(work)
        return work / "oxidgene-desktop.exe"
    if name.endswith(".dmg"):
        mount = work / "image"
        subprocess.run(
            ["hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", str(mount), str(archive)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        try:
            shutil.copytree(mount / "OxidGene.app", work / "OxidGene.app", symlinks=True)
        finally:
            subprocess.run(["hdiutil", "detach", str(mount)], check=True, stdout=subprocess.DEVNULL)
        return work / "OxidGene.app" / "Contents" / "MacOS" / "oxidgene-desktop"
    fail(f"unknown download format: {name}")
    raise AssertionError


def check_version(executable: Path, version: str) -> None:
    output = subprocess.run(
        [str(executable), "--version"], capture_output=True, text=True, timeout=TIMEOUT, check=False
    )
    if output.returncode != 0 or output.stdout.strip() != f"oxidgene-desktop {version}":
        fail(f"--version answered {output.stdout.strip()!r} (exit {output.returncode}), expected {version}")
    print(f"ok: --version is {version}")


def check_universal(executable: Path) -> None:
    archs = subprocess.run(["lipo", "-archs", str(executable)], capture_output=True, text=True, check=True)
    found = set(archs.stdout.split())
    if not {"arm64", "x86_64"} <= found:
        fail(f"the executable is not universal: {' '.join(sorted(found))}")
    print("ok: universal executable (arm64, x86_64)")


def data_directory(work: Path, env: dict[str, str]) -> Path:
    """Where the executable will look for its database, `env` redirected
    there when the platform reads its location from the environment."""
    system = platform.system()
    if system == "Linux":
        env["XDG_DATA_HOME"] = str(work / "data")
        return work / "data" / "oxidgene"
    if system == "Darwin":
        env["HOME"] = str(work / "home")
        return work / "home" / "Library" / "Application Support" / "oxidgene"
    if system == "Windows":
        return Path(os.environ["APPDATA"]) / "oxidgene"
    fail(f"unsupported platform: {system}")
    raise AssertionError


def check_mcp(executable: Path, work: Path) -> None:
    env = {key: value for key, value in os.environ.items() if key != "OTEL_EXPORTER_OTLP_ENDPOINT"}
    env["OXIDGENE_LOG_LEVEL"] = "info"
    data = data_directory(work, env)
    database = data / "oxidgene.db"
    if database.exists():
        fail(f"a database is already at {database}: run this on a disposable machine")
    created = not data.exists()
    data.mkdir(parents=True, exist_ok=True)
    # An empty file stands for a database the application created: the
    # subcommand applies the migrations to it.
    database.touch()
    messages = [
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "desktop-smoke", "version": "0"},
            },
        },
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "list_trees", "arguments": {}}},
    ]
    try:
        session = subprocess.Popen(
            [str(executable), "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
        )
        # A session that hangs is killed, which ends the reading below.
        watchdog = threading.Timer(TIMEOUT, session.kill)
        watchdog.daemon = True
        watchdog.start()
        # The logs, read as they come so that a full pipe never stalls the
        # session.
        logs: list[bytes] = []
        drain = threading.Thread(target=lambda: logs.extend(session.stderr), daemon=True)
        drain.start()
        # Bytes, not text: a text pipe on Windows would end each line with
        # \r\n, which a client never sends.
        for message in messages:
            session.stdin.write(json.dumps(message).encode() + b"\n")
        session.stdin.flush()
        answer = None
        for raw in session.stdout:
            line = raw.decode("utf-8", errors="replace")
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                session.kill()
                fail(f"standard output carries a line that is no protocol message: {line.strip()!r}")
            if message.get("jsonrpc") != "2.0":
                session.kill()
                fail(f"not a JSON-RPC 2.0 message: {line.strip()!r}")
            if message.get("id") == 1 and "error" in message:
                session.kill()
                fail(f"initialize failed: {message['error']}")
            if message.get("id") == 2:
                answer = message
                break
        # Closing the input ends the session.
        session.stdin.close()
        try:
            stdout_rest = session.stdout.read().decode("utf-8", errors="replace")
            session.wait(timeout=TIMEOUT)
            drain.join(timeout=TIMEOUT)
            stderr = b"".join(logs).decode("utf-8", errors="replace")
        except subprocess.TimeoutExpired:
            session.kill()
            fail("the session did not end when its input closed")
        finally:
            watchdog.cancel()
        if answer is None:
            fail(f"list_trees got no answer (exit {session.returncode}): {stderr.strip()[-2000:]}")
        content = answer.get("result", {}).get("structuredContent", {})
        if content.get("total_count") != 0:
            fail(f"list_trees answered {json.dumps(answer)[:500]}, expected the empty list of a fresh database")
        if stdout_rest.strip():
            fail(f"standard output carries more than the protocol: {stdout_rest.strip()[:500]!r}")
        if session.returncode != 0:
            fail(f"the session exited with {session.returncode}: {stderr.strip()[-2000:]}")
        print("ok: MCP session over stdio, list_trees round trip")
    finally:
        if platform.system() == "Windows":
            if created:
                shutil.rmtree(data, ignore_errors=True)
            else:
                for leftover in data.glob("oxidgene.db*"):
                    leftover.unlink(missing_ok=True)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    archive = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="oxidgene-smoke-") as directory:
        work = Path(directory)
        executable = unpack(archive, work)
        if not executable.is_file():
            fail(f"{archive.name} holds no {executable.name}")
        check_version(executable, workspace_version())
        if platform.system() == "Darwin":
            check_universal(executable)
        check_mcp(executable, work)
    print(f"desktop smoke: {archive.name} passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
