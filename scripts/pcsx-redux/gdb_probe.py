#!/usr/bin/env python3
"""GDB-stub bridge for one-shot probes against PCSX-Redux.

PCSX-Redux exposes a GDB Remote Serial Protocol stub when started with
the debugger enabled (defaults to TCP port 3333 in its settings). This
script speaks the protocol directly so you can perform ad-hoc reads
without authoring a full Lua autorun + state machine.

When to reach for this vs `.probe.toml`:
  * Use `.probe.toml` for repeatable captures that produce a CSV (the
    state machine handles save-state load -> arm bps -> dump). The
    output is reproducible and `probe.py regress` can gate on it.
  * Use `gdb-probe.py` for *one-shot* investigations: "read 512 bytes
    at X right now", "step past this instruction and dump register
    state", "show me memory the next time PC hits Y once". No CSV
    schema, no scenario manifest entry, no state machine to author.

Subcommands:
    read-mem  ADDR LEN  [--out FILE]      hex dump or raw bytes to FILE
    read-regs                              dump all 38 PSX GPRs + PC
    write-mem ADDR HEXBYTES                write hex-string of bytes to ADDR
    when-pc-hits ADDR --read-mem A,L [--out F]
                                           one-shot: arm exec BP, continue,
                                           on hit read A..A+L, disarm
    watch ADDR LEN --kind {read|write|access}
                                           insert watchpoint; print stop
                                           reply when it fires
    selftest                               run the protocol codec
                                           tests against a mock server
                                           (no live emulator needed)

ADDR accepts either hex (`0x801DE840`, `801de840`) or a Ghidra symbol
name (`FUN_801DD35C`, `_DAT_8007BCD0`) resolved via the same
ghidra/scripts/symbols.json the Lua probe layer uses.

Default host:port is 127.0.0.1:3333. PCSX-Redux's setting is
`Emulator -> GDB server port`; enable it before running.

Stdlib-only.
"""

from __future__ import annotations

import argparse
import json
import re
import socket
import struct
import sys
import threading
from pathlib import Path
from typing import Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
SYMBOLS_JSON = REPO_ROOT / "ghidra" / "scripts" / "symbols.json"


# ============================================================
# Symbol resolution

_symbols_cache: dict[str, int] | None = None


def load_symbols() -> dict[str, int]:
    """Load ghidra/scripts/symbols.json once. Empty dict if missing."""
    global _symbols_cache
    if _symbols_cache is not None:
        return _symbols_cache
    if not SYMBOLS_JSON.exists():
        _symbols_cache = {}
        return _symbols_cache
    with open(SYMBOLS_JSON, "r", encoding="utf-8") as f:
        payload = json.load(f)
    _symbols_cache = {k: int(v, 16) for k, v in payload.get("symbols", {}).items()}
    return _symbols_cache


def resolve_addr(spec: str) -> int:
    """Accept hex (0x...) or a symbol name; return int.

    Symbol misses raise SystemExit with the same regenerate-via hint
    that the Lua probe layer uses, so the user gets a coherent message
    whichever tool surfaced the typo.
    """
    s = spec.strip()
    if not s:
        raise SystemExit("gdb-probe: empty address")
    if re.fullmatch(r"(?:0x)?[0-9a-fA-F]+", s):
        return int(s, 16)
    syms = load_symbols()
    if s in syms:
        return syms[s]
    norm = re.sub(r"([_A-Za-z])([0-9A-Fa-f]+)$",
                  lambda m: m.group(1) + m.group(2).lower(), s)
    if norm in syms:
        return syms[norm]
    raise SystemExit(
        f"gdb-probe: cannot resolve '{spec}' as hex or as a Ghidra symbol. "
        f"Regenerate ghidra/scripts/symbols.json via "
        "scripts/pcsx-redux/build-symbols.py.")


# ============================================================
# GDB Remote Serial Protocol codec (pure functions, testable
# independently of a live socket)


def packet_checksum(data: bytes) -> int:
    """Lower 8 bits of the sum of all data bytes (GDB RSP spec)."""
    return sum(data) & 0xFF


def make_packet(data: bytes) -> bytes:
    """Wrap a payload into a `$<data>#<checksum>` frame."""
    return b"$" + data + b"#" + f"{packet_checksum(data):02x}".encode()


def parse_packet(buf: bytes) -> tuple[bytes | None, bytes]:
    """Pull one well-formed packet out of `buf`.

    Returns ``(payload, remainder)``. ``payload`` is None if `buf`
    doesn't yet contain a complete packet; in that case the caller
    should ``recv()`` more bytes and try again. Invalid checksums
    raise.
    """
    # Skip leading acks ('+'/'-') that the peer may emit between packets.
    i = 0
    while i < len(buf) and buf[i:i + 1] in (b"+", b"-"):
        i += 1
    if i >= len(buf):
        return None, b""
    if buf[i:i + 1] != b"$":
        # Unexpected byte. Strip and retry rather than failing - some
        # stubs interleave OOB output.
        return None, buf[i + 1:]
    # Find the '#'.
    hash_at = buf.find(b"#", i + 1)
    if hash_at < 0 or hash_at + 2 >= len(buf):
        return None, buf[i:]  # incomplete; keep the partial
    payload = buf[i + 1:hash_at]
    cs_chars = buf[hash_at + 1:hash_at + 3]
    if not re.fullmatch(rb"[0-9a-fA-F]{2}", cs_chars):
        raise ValueError(f"bad checksum bytes {cs_chars!r}")
    cs_recv = int(cs_chars.decode(), 16)
    cs_calc = packet_checksum(payload)
    if cs_recv != cs_calc:
        raise ValueError(
            f"checksum mismatch: got 0x{cs_recv:02x}, want 0x{cs_calc:02x}"
            f" for payload {payload[:40]!r}")
    return payload, buf[hash_at + 3:]


# ============================================================
# Socket client wrapper


class GdbStub:
    """Minimal RSP client. Synchronous, single-threaded.

    Lifecycle:
        stub = GdbStub("127.0.0.1", 3333); stub.connect()
        stub.read_mem(0x80000000, 4096)
        stub.close()
    """

    def __init__(self, host: str = "127.0.0.1", port: int = 3333,
                 timeout: float = 5.0):
        self.host = host
        self.port = port
        self.timeout = timeout
        self.sock: socket.socket | None = None
        self.no_ack = False
        self._recv_buf = b""

    def connect(self) -> None:
        self.sock = socket.create_connection((self.host, self.port),
                                              timeout=self.timeout)
        # Some stubs send '+' as the initial handshake. Drain it.
        try:
            self.sock.settimeout(0.1)
            initial = self.sock.recv(64)
            self._recv_buf += initial
        except (TimeoutError, socket.timeout):
            pass
        finally:
            self.sock.settimeout(self.timeout)
        # Best-effort no-ack negotiation. Older stubs ignore.
        resp = self._exchange(b"QStartNoAckMode")
        self.no_ack = (resp == b"OK")

    def close(self) -> None:
        if self.sock is not None:
            try:
                self.sock.close()
            finally:
                self.sock = None

    def _send_raw(self, data: bytes) -> None:
        assert self.sock is not None
        self.sock.sendall(data)

    def _send_packet(self, payload: bytes) -> None:
        self._send_raw(make_packet(payload))
        if self.no_ack:
            return
        # Wait for '+' ack; retry on '-' (NAK).
        while True:
            b = self._recv_raw(1)
            if b == b"+":
                return
            if b == b"-":
                self._send_raw(make_packet(payload))
                continue
            # Some stubs may emit packet data interleaved with ack; if so
            # buffer it for the upcoming _recv_packet().
            self._recv_buf += b

    def _recv_raw(self, n: int) -> bytes:
        assert self.sock is not None
        if self._recv_buf:
            head, self._recv_buf = self._recv_buf[:n], self._recv_buf[n:]
            if len(head) >= n:
                return head
            n -= len(head)
            tail = self.sock.recv(n)
            return head + tail
        return self.sock.recv(n)

    def _recv_packet(self) -> bytes:
        while True:
            pkt, rest = parse_packet(self._recv_buf)
            if pkt is not None:
                self._recv_buf = rest
                if not self.no_ack:
                    self._send_raw(b"+")
                return pkt
            self._recv_buf = rest + self._recv_raw(1024)

    def _exchange(self, payload: bytes) -> bytes:
        self._send_packet(payload)
        return self._recv_packet()

    # ---------- High-level operations ----------

    def read_mem(self, addr: int, length: int) -> bytes:
        """Read `length` bytes from `addr`. Chunks above 0x400."""
        out = bytearray()
        cur, remaining = addr, length
        while remaining > 0:
            chunk = min(remaining, 0x400)
            resp = self._exchange(f"m{cur:x},{chunk:x}".encode())
            if resp.startswith(b"E"):
                raise IOError(
                    f"gdb-probe: read error at 0x{cur:08x}: {resp.decode()}")
            try:
                out.extend(bytes.fromhex(resp.decode()))
            except ValueError as e:
                raise IOError(
                    f"gdb-probe: malformed read reply at 0x{cur:08x}: "
                    f"{resp!r}") from e
            cur += chunk
            remaining -= chunk
        return bytes(out)

    def write_mem(self, addr: int, data: bytes) -> None:
        hex_str = data.hex()
        resp = self._exchange(f"M{addr:x},{len(data):x}:{hex_str}".encode())
        if resp != b"OK":
            raise IOError(f"gdb-probe: write error at 0x{addr:08x}: {resp.decode()}")

    def read_regs(self) -> bytes:
        """Return the raw hex-encoded register block.

        Layout is target-specific; PSX MIPS exposes 38 32-bit regs in
        the standard order (r0..r31, sr, lo, hi, bad, cause, pc).
        Caller is responsible for slicing.
        """
        resp = self._exchange(b"g")
        if resp.startswith(b"E"):
            raise IOError(f"gdb-probe: regs error: {resp.decode()}")
        return resp

    def insert_bp(self, kind: int, addr: int, length: int = 4) -> None:
        """`kind`: 0=sw, 1=hw exec, 2=write-watch, 3=read-watch, 4=access-watch."""
        resp = self._exchange(f"Z{kind},{addr:x},{length:x}".encode())
        if resp != b"OK":
            raise IOError(
                f"gdb-probe: insert BP kind={kind} at 0x{addr:08x} failed: {resp.decode()}")

    def remove_bp(self, kind: int, addr: int, length: int = 4) -> None:
        # Best-effort; ignore failure (BP may already be cleared).
        try:
            self._exchange(f"z{kind},{addr:x},{length:x}".encode())
        except IOError:
            pass

    def continue_until_stop(self) -> bytes:
        """Send 'c', return the stop-reply packet (T05 ...)."""
        # Continue has no immediate response - the stub blocks until
        # the next stop event. Bump socket timeout so long-running
        # programs aren't cut off; caller can adjust via .timeout.
        self._send_packet(b"c")
        assert self.sock is not None
        old = self.sock.gettimeout()
        try:
            self.sock.settimeout(None)
            return self._recv_packet()
        finally:
            self.sock.settimeout(old)


# ============================================================
# CLI


def _parse_hexbytes(s: str) -> bytes:
    s = s.strip().lower().removeprefix("0x")
    if len(s) % 2:
        raise SystemExit(f"gdb-probe: hex bytes must be even-length: {s!r}")
    return bytes.fromhex(s)


def _print_hexdump(data: bytes, base: int) -> None:
    """16-byte/line, mixed hex + ASCII, GDB-style."""
    for i in range(0, len(data), 16):
        chunk = data[i:i + 16]
        hex_part = " ".join(f"{b:02x}" for b in chunk)
        ascii_part = "".join(chr(b) if 0x20 <= b < 0x7F else "." for b in chunk)
        print(f"{base + i:08x}  {hex_part:<48}  |{ascii_part}|")


def cmd_read_mem(args) -> int:
    addr = resolve_addr(args.addr)
    stub = GdbStub(args.host, args.port)
    stub.connect()
    try:
        data = stub.read_mem(addr, args.length)
    finally:
        stub.close()
    if args.out:
        Path(args.out).write_bytes(data)
        print(f"wrote {len(data)} bytes to {args.out}")
    else:
        _print_hexdump(data, addr)
    return 0


def cmd_read_regs(args) -> int:
    stub = GdbStub(args.host, args.port)
    stub.connect()
    try:
        hex_blob = stub.read_regs().decode()
    finally:
        stub.close()
    # 32 GPRs + sr, lo, hi, bad, cause, pc = 38 little-endian 32-bit regs.
    # Note: GDB hex order is target-byte-order; PSX MIPS GDB stubs use LE.
    names = [f"r{i}" for i in range(32)] + ["sr", "lo", "hi", "bad", "cause", "pc"]
    raw = bytes.fromhex(hex_blob)
    for i, name in enumerate(names):
        if (i + 1) * 4 > len(raw):
            break
        val = struct.unpack("<I", raw[i * 4:(i + 1) * 4])[0]
        print(f"  {name:<5} = 0x{val:08x}")
    return 0


def cmd_write_mem(args) -> int:
    addr = resolve_addr(args.addr)
    data = _parse_hexbytes(args.bytes)
    stub = GdbStub(args.host, args.port)
    stub.connect()
    try:
        stub.write_mem(addr, data)
    finally:
        stub.close()
    print(f"wrote {len(data)} bytes to 0x{addr:08x}")
    return 0


def cmd_when_pc_hits(args) -> int:
    bp_addr = resolve_addr(args.addr)
    read_addr_str, read_len_str = args.read_mem.split(",")
    read_addr = resolve_addr(read_addr_str)
    read_len = int(read_len_str, 0)

    stub = GdbStub(args.host, args.port)
    stub.connect()
    try:
        stub.insert_bp(1, bp_addr)
        try:
            stop = stub.continue_until_stop()
            print(f"hit: {stop.decode(errors='replace')}")
            data = stub.read_mem(read_addr, read_len)
        finally:
            stub.remove_bp(1, bp_addr)
    finally:
        stub.close()

    if args.out:
        Path(args.out).write_bytes(data)
        print(f"wrote {len(data)} bytes to {args.out}")
    else:
        _print_hexdump(data, read_addr)
    return 0


def cmd_watch(args) -> int:
    addr = resolve_addr(args.addr)
    kind_map = {"write": 2, "read": 3, "access": 4}
    kind = kind_map[args.kind]
    stub = GdbStub(args.host, args.port)
    stub.connect()
    try:
        stub.insert_bp(kind, addr, args.length)
        try:
            stop = stub.continue_until_stop()
            print(f"watchpoint fired: {stop.decode(errors='replace')}")
        finally:
            stub.remove_bp(kind, addr, args.length)
    finally:
        stub.close()
    return 0


# ============================================================
# In-process protocol selftest (no live emulator needed)


def _selftest_codec() -> None:
    # round-trip a few payloads
    cases = [b"OK", b"m80000000,4", b"E01", b"T05thread:01;", b""]
    for c in cases:
        framed = make_packet(c)
        parsed, rest = parse_packet(framed)
        assert parsed == c, f"round-trip failed for {c!r}: got {parsed!r}"
        assert rest == b"", f"unexpected remainder: {rest!r}"

    # buffered with leading ack
    buf = b"+" + make_packet(b"OK") + b"+"
    parsed, rest = parse_packet(buf)
    assert parsed == b"OK"
    assert rest == b"+"

    # split packet: first call returns None, second returns the payload
    full = make_packet(b"abcdef")
    half = full[:len(full) // 2]
    parsed, rest = parse_packet(half)
    assert parsed is None
    assert rest == half  # caller keeps trying

    # bad checksum should raise
    bad = b"$OK#ff"
    try:
        parse_packet(bad)
    except ValueError as e:
        assert "checksum" in str(e), str(e)
    else:
        raise AssertionError("expected ValueError on bad checksum")


def _selftest_client() -> None:
    """Spin up a mock GDB stub on localhost, exercise the client.

    The mock responds to:
        QStartNoAckMode -> OK   (we then go ack-less for the rest)
        m<addr>,<len>   -> hex bytes (addr & 0xff repeated len times)
        g               -> 38 LE u32s; r4 = 0xDEADBEEF; pc = 0x801DE840
        Z1,<addr>,<len> -> OK
        z1,<addr>,<len> -> OK
        c               -> immediately replies with T05 (BP hit)
    """
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.bind(("127.0.0.1", 0))
    srv.listen(1)
    port = srv.getsockname()[1]

    def handle():
        conn, _ = srv.accept()
        no_ack = False
        buf = b""
        try:
            while True:
                data = conn.recv(4096)
                if not data:
                    return
                buf += data
                while True:
                    pkt, rest = parse_packet(buf)
                    if pkt is None:
                        break
                    buf = rest
                    if not no_ack:
                        conn.sendall(b"+")
                    # dispatch
                    if pkt == b"QStartNoAckMode":
                        conn.sendall(make_packet(b"OK"))
                        no_ack = True
                    elif pkt.startswith(b"m"):
                        addr_hex, len_hex = pkt[1:].split(b",")
                        addr = int(addr_hex, 16)
                        n = int(len_hex, 16)
                        payload_bytes = bytes([addr & 0xFF]) * n
                        conn.sendall(make_packet(payload_bytes.hex().encode()))
                    elif pkt == b"g":
                        words = [0] * 38
                        words[4] = 0xDEADBEEF
                        words[37] = 0x801DE840
                        raw = b"".join(struct.pack("<I", w) for w in words)
                        conn.sendall(make_packet(raw.hex().encode()))
                    elif pkt.startswith(b"Z") or pkt.startswith(b"z"):
                        conn.sendall(make_packet(b"OK"))
                    elif pkt == b"c":
                        conn.sendall(make_packet(b"T05thread:01;"))
                    else:
                        conn.sendall(make_packet(b""))  # unsupported
        finally:
            conn.close()

    t = threading.Thread(target=handle, daemon=True)
    t.start()

    stub = GdbStub("127.0.0.1", port, timeout=2.0)
    stub.connect()
    try:
        assert stub.no_ack, "no-ack negotiation should succeed against mock"
        data = stub.read_mem(0x801DE800, 16)
        assert data == bytes([0x00]) * 16, f"unexpected mem: {data!r}"
        regs = stub.read_regs()
        raw = bytes.fromhex(regs.decode())
        r4 = struct.unpack("<I", raw[16:20])[0]
        assert r4 == 0xDEADBEEF, f"r4 mismatch: 0x{r4:08x}"
        stub.insert_bp(1, 0x801DE840)
        stop = stub.continue_until_stop()
        assert stop.startswith(b"T05"), f"unexpected stop: {stop!r}"
        stub.remove_bp(1, 0x801DE840)
    finally:
        stub.close()
        srv.close()


def cmd_selftest(args) -> int:
    _selftest_codec()
    print("codec self-test: OK")
    _selftest_client()
    print("client self-test (mock server): OK")
    return 0


# ============================================================
# Entry point


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="gdb-probe.py",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", default=3333, type=int)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_rd = sub.add_parser("read-mem", help="read LEN bytes from ADDR")
    p_rd.add_argument("addr", help="hex (0x801DE840) or symbol name (FUN_801DD35C)")
    p_rd.add_argument("length", type=lambda s: int(s, 0))
    p_rd.add_argument("--out", help="write raw bytes here instead of hexdump")
    p_rd.set_defaults(fn=cmd_read_mem)

    p_rg = sub.add_parser("read-regs", help="dump 38 PSX MIPS GPRs + PC")
    p_rg.set_defaults(fn=cmd_read_regs)

    p_wr = sub.add_parser("write-mem", help="write hex-string of bytes to ADDR")
    p_wr.add_argument("addr")
    p_wr.add_argument("bytes", help="even-length hex string, e.g. 0xDEADBEEF")
    p_wr.set_defaults(fn=cmd_write_mem)

    p_pc = sub.add_parser("when-pc-hits",
                          help="arm exec BP, continue, read mem on hit, disarm")
    p_pc.add_argument("addr", help="BP address (hex or symbol)")
    p_pc.add_argument("--read-mem", required=True, metavar="ADDR,LEN",
                      help="memory region to dump on the BP fire")
    p_pc.add_argument("--out", help="write raw bytes here")
    p_pc.set_defaults(fn=cmd_when_pc_hits)

    p_wt = sub.add_parser("watch", help="insert watchpoint; print stop on fire")
    p_wt.add_argument("addr")
    p_wt.add_argument("length", type=lambda s: int(s, 0))
    p_wt.add_argument("--kind", choices=("read", "write", "access"),
                      default="access")
    p_wt.set_defaults(fn=cmd_watch)

    p_st = sub.add_parser("selftest",
                          help="exercise codec + client against a mock server")
    p_st.set_defaults(fn=cmd_selftest)

    args = ap.parse_args(argv)
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
