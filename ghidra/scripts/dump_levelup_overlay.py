# @category Legaia
# @runtime Jython
#
# Dumps all 79 functions from the level-up / battle overlay (overlay_magic_level_up.bin).
# The level-up capture (mc3) loads the battle overlay, which hosts battle logic,
# battle action SM, level-up sequencer, and XP/stat-gain calculations.
#
# Key unknowns to surface:
#   - 801d0748 (11KB, 0 incoming) -- root entry: level-up sequence dispatcher
#   - 801d388c (7.8KB, 39 callers) -- XP processing / stat-gain handler
#   - 801d5854 (6.5KB, 47 callers) -- animation / display driver
#   - 801dc0a0 (3.6KB, 7 callers)  -- stat display / confirmation screen
#   - 801ddb30 (3.5KB, 3 callers)  -- post-battle summary / results
#   - 801e295c                     -- battle action SM (already dumped in overlay_battle_action)
#
# Run against the named overlay program:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_magic_level_up.bin -noanalysis \
#       -postScript /scripts/dump_levelup_overlay.py
#
# Output files land in /scripts/funcs/overlay_magic_level_up_<addr>.txt

from lib_dump import dump_targets

TARGETS = [
    # Top by incoming xref count (primary dispatchers)
    "801d8de8",  # 75 callers (3028 bytes) -- UI element ID mapper (FUN_801D8DE8)
    "801d5854",  # 47 callers (6500 bytes) -- animation / display driver
    "801d388c",  # 39 callers (7820 bytes) -- XP processing / stat-gain
    "801d829c",  # 24 callers (548 bytes)
    "801d5718",  # 18 callers (96 bytes)
    "801db81c",  # 10 callers (152 bytes)
    "801da6b4",  # 9 callers (204 bytes)
    "801d8d00",  # 9 callers (232 bytes)
    "801db8b4",  # 8 callers (64 bytes)
    "801d99bc",  # 8 callers (300 bytes)
    "801dc0a0",  # 7 callers (3596 bytes) -- stat display screen
    "801dfdf0",  # 6 callers (656 bytes)
    "801dceac",  # 6 callers (512 bytes)
    "801db8f4",  # 6 callers (208 bytes)
    "801d8a88",  # 6 callers (632 bytes)
    "801d32bc",  # 6 callers (392 bytes)
    "801e2650",  # 4 callers (780 bytes)
    "801dbddc",  # 4 callers (232 bytes)
    "801e1d98",  # 3 callers (1328 bytes)
    "801ddb30",  # 3 callers (3556 bytes) -- post-battle results
    "801dd864",  # 3 callers (716 bytes)
    "801dbb8c",  # (212 bytes)
    "801dba04",  # (148 bytes)
    "801daba4",  # (52 bytes)
    # Higher address cluster (XP table / stat functions?)
    "801efe44",
    "801eed1c",
    "801dbc30",
    "801db124",
    "801da780",
    "801da34c",
    "801d88cc",
    "801d57e8",
    "801d5778",
    "801efbfc",
    "801ef9e4",
    "801ec0dc",
    "801e9fd4",
    "801e93c8",
    "801e92dc",
    "801e91e8",
    "801e805c",
    "801e791c",
    "801e7824",
    "801e752c",
    "801e7320",
    "801e7250",
    "801e70bc",
    "801e6d84",
    "801e6968",
    "801e1ab0",
    "801df570",
    "801dd0ac",
    "801dbf9c",
    "801dbec4",
    "801dbd04",
    "801db9c4",
    "801db7b0",  # cited by FUN_801D8DE8 (JT dispatcher)
    "801db318",
    "801da59c",
    "801d9d3c",
    "801d9bbc",
    "801d9ae8",
    "801d84c0",
    "801d71b8",
    "801d3748",
    "801d3444",
    "801ec3e4",
    "801e9504",
    "801e295c",  # battle action SM (already in overlay_battle_action, dump for cross-check)
    "801e2524",
    "801e22c8",
    "801e09f8",
    "801df6b8",
    "801dea50",
    "801dd6b4",
    "801dd4b0",
    "801dba90",
    "801d0748",  # ROOT: 11124 bytes, 0 incoming -- level-up sequence entry
]

dump_targets(currentProgram, TARGETS, scus_bare=True)
