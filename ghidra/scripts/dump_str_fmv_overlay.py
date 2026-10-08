# @category Legaia
# @runtime Jython
#
# Dumps functions from the STR/MDEC FMV overlay loaded during pre-rendered video playback.
# This overlay handles game modes 26 (StrInit) and 27 (StrMode) - it is distinct from
# overlay_cutscene_dialogue.bin (which covers actor-scripted story cutscenes).
#
# To capture the overlay:
#   1. Boot mednafen with the disc and advance to any pre-rendered FMV (opening movie,
#      ending movie, or a CDNAME scene whose label maps to an MV*.STR entry).
#   2. Save state while the video is playing:
#         mednafen -> F5 to save into slot N
#   3. Extract the overlay slice:
#         scripts/ghidra-analysis/analyze-overlay.sh ~/.mednafen/mcs/Legend*Legaia*.mcN --label str_fmv
#   4. Import into Ghidra:
#         scripts/ghidra-analysis/import-overlay-named.sh ghidra/projects/legaia.rep str_fmv
#   5. Run this script:
#         docker compose exec ghidra /ghidra/support/analyzeHeadless \
#             /projects legaia -process overlay_str_fmv.bin -noanalysis \
#             -postScript /scripts/dump_str_fmv_overlay.py
#
# Key unknowns to look for in the STR/MDEC overlay:
#   - StrInit handler: sets up MDEC hardware, opens CD stream, starts XA channel.
#   - StrMode per-frame loop: CdReadFile_chunk -> StrFrameAssembler -> MdecDecoder ->
#     blit full-screen texture.
#   - XA channel selector: maps (file_no, ch_no) to the cutscene name / CDNAME label.
#   - CDNAME-to-STR entry table: maps op*/ed* scene names to MV*.STR disc paths.
#   - libcd cluster: CdControl_raw (0x8005D9A0), CdReadFile_chunk (0x8005E4D4),
#     CdSync (0x8005CCB4) - look for callers.
#
# Output files land in /scripts/funcs/overlay_str_fmv_<addr>.txt
# (TARGETS is initially empty; populate from the inventory CSV once captured.)

from lib_dump import dump_targets

TARGETS = [
    "0x801CFFDC",  # 5 incoming
    "0x801CFB94",  # 3 incoming
    "0x801D0248",  # 2 incoming, 304 bytes
    "0x801CFA14",  # 2 incoming, 192 bytes
    "0x801D0100",  # 2 incoming, 152 bytes
    "0x801D0198",  # 2 incoming, 152 bytes
    "0x801CFEBC",  # 2 incoming, 36 bytes
    "0x801CFE00",  # 2 incoming, 32 bytes
    "0x801D0230",  # 2 incoming, 24 bytes
    "0x801CF098",  # 1 incoming, 1236 bytes (largest, root caller)
    "0x801D070C",  # 1 incoming, 828 bytes
    "0x801D0378",  # 1 incoming, 652 bytes
    "0x801CF740",  # 1 incoming, 368 bytes
    "0x801CFEE0",  # 1 incoming, 252 bytes
    "0x801F1A00",  # 1 incoming, 232 bytes (out-of-cluster helper)
    "0x801CF8B0",  # 1 incoming, 216 bytes
    "0x801D0604",  # 1 incoming, 212 bytes
    "0x801CFAD4",  # 1 incoming, 192 bytes
    "0x801D0070",  # 1 incoming, 144 bytes
    "0x801CF988",  # 1 incoming, 140 bytes
    "0x801CFD84",  # 1 incoming, 124 bytes
    "0x801CFC18",  # 1 incoming, 56 bytes
    "0x801CF56C",  # 0 incoming (root entry candidate, 468 bytes)
    "0x801CFCDC",  # 0 incoming (156 bytes)
    "0x801CFE20",  # 0 incoming (60 bytes)
    "0x801CFE5C",  # 0 incoming (60 bytes)
    "0x801CFE98",  # 0 incoming (36 bytes)
]

dump_targets(currentProgram, TARGETS, scus_bare=True)
