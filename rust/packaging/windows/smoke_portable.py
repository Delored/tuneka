#!/usr/bin/env python3
"""Exercise packaged Windows CLI/MCP without a development-tool PATH."""

import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time


def check_portable(stage: Path) -> dict:
    if os.name != "nt":
        raise RuntimeError("Portable Windows runtime checks require a Windows host")
    stage = stage.resolve(strict=True)
    env = os.environ.copy()
    windows = Path(env["WINDIR"])
    env["PATH"] = os.pathsep.join((str(windows / "System32"), str(windows)))
    for name in ("FFMPEG_DIR", "VCPKG_ROOT", "VCPKGRS_DYNAMIC", "LIBCLANG_PATH"):
        env.pop(name, None)
    with tempfile.TemporaryDirectory(prefix="tuneka-portable-") as scratch:
        env["TUNEKA_RUST_DATA_DIR"] = str(Path(scratch) / "state")
        env["TUNEKA_DOWNLOADS"] = str(Path(scratch) / "outputs")
        version = subprocess.run(
            [str(stage / "tuneka.exe"), "--version"],
            cwd=scratch, env=env, capture_output=True, text=True, check=True,
            timeout=30,
        ).stdout.strip()
        if not version.startswith("tuneka "):
            raise RuntimeError(f"Unexpected CLI version output: {version!r}")
        process = subprocess.Popen(
            [str(stage / "tuneka-mcp.exe")], cwd=scratch, env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, encoding="utf-8",
        )
        messages = queue.Queue()
        diagnostics = []

        def read_messages():
            try:
                for line in process.stdout:
                    messages.put(json.loads(line))
            except Exception as error:
                messages.put(error)
            finally:
                messages.put(None)

        def read_diagnostics():
            diagnostics.extend(process.stderr)

        reader = threading.Thread(target=read_messages, daemon=True)
        errors = threading.Thread(target=read_diagnostics, daemon=True)
        reader.start()
        errors.start()

        def send(message):
            process.stdin.write(json.dumps(message) + "\n")
            process.stdin.flush()

        def request(identifier, method, params):
            send({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params})
            deadline = time.monotonic() + 30
            while True:
                message = messages.get(timeout=max(0.01, deadline - time.monotonic()))
                if isinstance(message, Exception):
                    raise message
                if message is None:
                    raise RuntimeError("MCP exited before its response: " + "".join(diagnostics))
                if message.get("id") != identifier:
                    if time.monotonic() >= deadline:
                        raise TimeoutError(f"MCP timed out handling {method}")
                    continue
                if "error" in message:
                    raise RuntimeError(f"MCP {method} failed: {message['error']}")
                return message["result"]

        try:
            initialized = request(1, "initialize", {
                "protocolVersion": "2025-03-26", "capabilities": {},
                "clientInfo": {"name": "tuneka-portable-smoke", "version": "1"},
            })
            if "serverInfo" not in initialized or "protocolVersion" not in initialized:
                raise RuntimeError("MCP initialize response is incomplete")
            send({"jsonrpc": "2.0", "method": "notifications/initialized"})
            tools = request(2, "tools/list", {})["tools"]
            expected = {
                "inspect_media", "list_formats", "list_playlist_items", "create_download",
                "get_job", "list_jobs", "pause_job", "resume_job", "cancel_job",
            }
            if {tool["name"] for tool in tools} != expected:
                raise RuntimeError("Packaged MCP server exposes unexpected tools")
            if any(tool.get("inputSchema", {}).get("type") != "object" for tool in tools):
                raise RuntimeError("Packaged MCP tool input schemas are incomplete")
            jobs = request(3, "tools/call", {"name": "list_jobs", "arguments": {}})
            if jobs.get("isError", False) or jobs.get("structuredContent") != []:
                raise RuntimeError(f"Packaged MCP list_jobs failed: {jobs!r}")
            process.stdin.close()
            code = process.wait(timeout=30)
            reader.join(timeout=5)
            errors.join(timeout=5)
            if code != 0:
                raise RuntimeError(f"MCP EOF shutdown returned {code}: {''.join(diagnostics)}")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
            for stream in (process.stdin, process.stdout, process.stderr):
                stream.close()
        return {
            "target": "x86_64-pc-windows-msvc",
            "cli_version": version,
            "mcp_initialize_tools_list_jobs_eof": "passed",
            "mcp_protocol_version": initialized["protocolVersion"],
            "runtime_path": "Windows System32 and Windows directory only",
            "gui_interaction": "not run",
            "tui_console_interaction": "not run",
            "live_youtube": "not run",
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    arguments = parser.parse_args()
    evidence = check_portable(arguments.stage)
    arguments.evidence.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    main()
