#!/usr/bin/env python3
"""PTY interaction check of the real TUI binary; no display/network required."""
import os
import fcntl
import struct
from pathlib import Path
import pty
import select
import subprocess
import tempfile
import termios
import time

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='tuneka-tui-test-') as temp:
    master, slave = pty.openpty()
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",30,120,0,0))
    original = termios.tcgetattr(slave)
    environment = os.environ.copy()
    environment.update(TERM='xterm-256color', TUNEKA_RUST_DATA_DIR=temp, TUNEKA_DOWNLOADS=temp)
    process = subprocess.Popen([str(root / 'target/debug/tuneka-tui')], stdin=slave, stdout=slave, stderr=slave, env=environment)
    output = bytearray()
    def drain(seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.05)[0]:
                chunk=os.read(master,65536)
                output.extend(chunk)
                if b'\x1b[6n' in chunk:
                    os.write(master,b'\x1b[1;1R')
    try:
        drain(1.0)
        os.write(master, b'http://localhost/secret\r')
        drain(0.4)
        os.write(master, b'\tpc')
        drain(0.2)
        os.write(master, b'q')
        drain(0.3)
        assert process.wait(timeout=5) == 0
        assert b'\x1b[?1049h' in output, 'alternate screen was not entered'
        assert b'\x1b[?1049l' in output, 'alternate screen was not restored'
        assert b'HTTPS' in output and b'YouTube' in output, 'real input did not produce expected validation error'
        assert termios.tcgetattr(slave) == original, 'terminal flags were not restored'
        print('PASS: real TUI URL input, error, job controls, clean quit and terminal restoration')
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        (root / "artifacts").mkdir(exist_ok=True)
        (root / "artifacts/tui-pty.log").write_bytes(output)
        os.close(master)
        os.close(slave)
