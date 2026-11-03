"""Tier 16 rows: which site sources make up the minigames page, and the one
file among them that may hand decoded audio to Web Audio.

Analysis: `ui_host_drift.tiers.page_cues`.
"""

from ui_host_drift.roots import REPO

# --------------------------------------------------------------------------
# Tier 16 - minigame cues key the live SPU
# --------------------------------------------------------------------------
#
# Every cue on the minigames page keys a voice on the page's audio director, so
# it carries the tone's ADSR, the retail volume and pan and the reverb send, and
# resolves against the game's banks as the play hosts' cues do. The page's old
# shape was a cue decoded to PCM and handed to an `AudioBufferSourceNode` at a
# hand-set gain. It sounds like the game, so nothing fails, and it has none of
# the above. This tier rejects that shape in the page's sources.
MINIGAME_PAGE_SOURCES = (
    REPO / "site/_content/minigames.html",
    *sorted((REPO / "site/js").glob("minigame-*.js")),
)

# The offline BGM render is the one thing the page hands to Web Audio as a
# buffer: it mixes with the live SPU at the device (`MgBgm2`).
BGM_BUFFER_HOST = REPO / "site/js/minigame-bgm.js"
