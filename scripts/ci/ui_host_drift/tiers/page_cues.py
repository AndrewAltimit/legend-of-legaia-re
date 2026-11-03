"""Tier 16 analysis: does a minigames page source play a cue as decoded PCM
rather than key a voice on the live SPU?

Rows: `ui_host_drift.rows.page_cues`.
"""

import re

from ui_host_drift.roots import REPO
from ui_host_drift.rows.page_cues import BGM_BUFFER_HOST, MINIGAME_PAGE_SOURCES
from ui_host_drift.tiers.page_keys import BLOCK_COMMENT_RE, strip_comments

# A cue PCM export called from the page.
CUE_PCM_RE = re.compile(r"\.\w*_(?:sfx|spin|sting)_(?:pcm|rate)\s*\(")
# The Web Audio half of the same shape: a buffer built and played by hand.
BUFFER_PLAY_RE = re.compile(r"\.create(?:BufferSource|Buffer)\s*\(")


def page_cue_violations(src: str, bgm_host: bool) -> list[str]:
    """Every decoded-cue construct in `src`, comments stripped."""
    text = strip_comments(BLOCK_COMMENT_RE.sub(" ", src))
    hits = [m.group(0) for m in CUE_PCM_RE.finditer(text)]
    if not bgm_host:
        hits += [m.group(0) for m in BUFFER_PLAY_RE.finditer(text)]
    return hits


def check_page_cues() -> tuple[list[str], int]:
    """Tier 16. Returns `(problems, files_scanned)`."""
    problems: list[str] = []
    for path in MINIGAME_PAGE_SOURCES:
        src = path.read_text(encoding="utf-8")
        for hit in page_cue_violations(src, path == BGM_BUFFER_HOST):
            problems.append(
                f"PAGE CUE {path.relative_to(REPO)}: `{hit}` plays decoded audio "
                f"by hand. A minigame cue keys a voice on the live SPU "
                f"(`MgSpu.cue` / `minigame_sfx_cue`), which is what gives it the "
                f"tone's ADSR, the retail volume and the reverb send; only the "
                f"BGM render in js/minigame-bgm.js is a buffer."
            )
    return problems, len(MINIGAME_PAGE_SOURCES)
