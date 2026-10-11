"""One finding: the record every signal emits and the report ranks.
"""

from __future__ import annotations

# ---------------------------------------------------------------------------
# Findings
# ---------------------------------------------------------------------------


class Finding:
    __slots__ = ("signal", "key", "addr", "where", "rank", "lines")

    def __init__(self, signal: str, key: str, addr: str, where: str, rank: float,
                 lines: list[str]):
        self.signal = signal
        self.key = key
        self.addr = addr
        self.where = where
        self.rank = rank
        self.lines = lines
