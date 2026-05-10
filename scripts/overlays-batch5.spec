# Batch-5 mednafen save-state captures - 2026-05-09
#
# Six new slots covering area-loading transitions and an in-battle art
# performance, captured during the fifth post-#26 batch. Use to extract
# 192 KB overlay slices for cross-validation against the existing field /
# battle overlays:
#
#   scripts/sweep-overlays.sh scripts/overlays-batch5.spec
#
# Slots 1..3 land in the field overlay (119 SP prologues, 148 jr ra),
# byte-identical to the existing field captures except for transient
# scene state (per-actor coords, RNG, BGM cursor).
#
# Slots 4..6 land in the battle overlay (57 SP prologues, 97 jr ra),
# byte-identical to the existing battle captures except for actor
# pointer table state at 0x801C9000-0x801C97FF (per `_DAT_801C9370`
# table base). Useful for live-state diffs against `attack_chain`
# behaviour.
#
# Format: <save-state-path>  <label>
~/.mednafen/mcs/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc1	area_load_a
~/.mednafen/mcs/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc2	area_load_b
~/.mednafen/mcs/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc3	area_load_c
~/.mednafen/mcs/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc4	battle_intro2
~/.mednafen/mcs/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc5	battle_arts_view
~/.mednafen/mcs/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc6	battle_somersault
