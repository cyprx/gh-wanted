"""Offline PTY smoke test. Run in Linux after cargo build."""
import fcntl
import os
import pty
import re
import select
import struct
import subprocess
import termios
import time

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 120, 0, 0))
before = termios.tcgetattr(slave)

def attach():
    os.setsid()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

process = subprocess.Popen(["target/debug/gh-wanted", "--demo"], stdin=slave, stdout=slave, stderr=slave,
                           preexec_fn=attach, env={**os.environ, "TERM": "xterm-256color"})
transcript = bytearray()
screen = [[" "] * 120 for _ in range(32)]
row = column = 0

def render(data):
    global row, column
    for token in re.findall(r"\x1b\[[0-?]*[ -/]*[@-~]|[^\x1b]", data.decode("utf-8", errors="replace")):
        if token.startswith("\x1b["):
            command = token[-1]
            args = token[2:-1]
            if args.startswith("?"):
                continue
            numbers = [int(n) if n else 0 for n in args.split(";")]
            if command in "Hf":
                row = (numbers[0] or 1) - 1
                column = (numbers[1] if len(numbers) > 1 else 1) - 1
            elif command == "G": column = (numbers[0] or 1) - 1
            elif command == "A": row -= numbers[0] or 1
            elif command == "B": row += numbers[0] or 1
            elif command == "C": column += numbers[0] or 1
            elif command == "D": column -= numbers[0] or 1
            elif command == "J" and numbers[0] == 2:
                for line in screen: line[:] = [" "] * 120
            elif command == "K":
                for c in range(max(0, column), 120): screen[min(row, 31)][c] = " "
        elif token == "\r": column = 0
        elif token == "\n": row += 1
        elif token.isprintable():
            if 0 <= row < 32 and 0 <= column < 120: screen[row][column] = token
            column += 1
    return "\n".join("".join(line) for line in screen).encode()

def read_for(seconds=0.5):
    chunk = bytearray()
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if select.select([master], [], [], 0.05)[0]:
            data = os.read(master, 65536)
            chunk.extend(data)
            if b"\x1b[6n" in data:
                os.write(master, b"\x1b[1;1R")
    transcript.extend(chunk)
    return render(bytes(chunk))

def send(text):
    os.write(master, text.encode())
    return read_for()

try:
    assert b"DEMO" in read_for(1)
    send("t")
    send("priority\r")
    send("/")
    send("topic:rust tag:priority\r")
    issue_view = send("i")
    assert b"Improve keyboard navigation" in issue_view, issue_view
    send("/")
    send('label:"good first issue" unassigned\r')
    send("s")
    assert b"Focus saved" in send("Rust starters\r")
    assert b"Rust starters" in send("f")
    reopened = send("\r")
    assert b"Improve keyboard navigation" in reopened
    detail = send("\t")
    assert b"regression test" in detail or b"regression test" in transcript
    assert b"fictional" in send("o")
    print("\n".join("".join(line).rstrip() for line in screen))
    send("q")
    assert process.wait(timeout=3) == 0
    assert termios.tcgetattr(slave) == before, "Terminal settings were not restored"
    assert b"\x1b[?1049l" in transcript, "Alternate screen was not restored"
    print("PASS: tag -> repository filter -> issue filter -> save -> reopen -> details -> safe demo browser action -> quit; terminal restored")
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
    os.close(master)
    os.close(slave)
    os.makedirs("target", exist_ok=True)
    with open("target/milestone-2-pty.log", "wb") as output:
        output.write(transcript)
