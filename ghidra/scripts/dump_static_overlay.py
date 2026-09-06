# @category Legaia
# @runtime Jython
#
# Dumper for the STATICALLY extracted overlay images
# (`extracted/overlays/overlay_<label>_<entry>.bin`, bases in
# `crates/asset/data/static-overlays.toml`), imported one Ghidra program per
# PROT entry so a dump's filename carries the image identity the shared load
# base cannot - see docs/tooling/static-overlay-pipeline.md.
#
# The slot-B summon stagers and capture-class cast modules are why this exists.
# They contain NO internal `jal`: every call leaves for SCUS or the resident
# battle overlay, and the module's own arms are reached through a jump table at
# the image head. So Ghidra's auto-analysis finds at most the tail function and
# leaves the module's real entry - the one the pager jumps to - undisassembled.
#
# The entries are recovered from the bytes instead, by FRAME MATCHING: a
# function starts at `addiu sp, sp, -F` and ends at the first `jr ra` whose
# delay slot is `addiu sp, sp, +F` for the SAME F. RANGES below records that
# partition, so each dump covers a whole function body rather than the one
# basic block Ghidra's flow-following reaches before the dispatcher's `jr $v0`.
#
# A weaker rule - "the prologue and `jr ra` counts match and interleave, so
# function `i` runs from prologue `i` to prologue `i+1`" - holds on the first
# thirteen images and BREAKS on the rest of the band. Three shapes break it and
# none of them breaks frame matching: a frameless leaf (no prologue at all, e.g.
# PROT 0949's stager at 0x801F75BC), an early `jr ra` inside a body, and a
# `jr ra` word that is simply data in the image's tail (PROT 0906 at
# 0x801F8070, past the last real function). Where a module's own entry tables
# in PROT 0898 name an address the frame scan missed, the table wins: those two
# VAs are added to the start set, and a table VA whose first instruction is a
# branch (PROT 0922's stager, prologue in the delay slot) suppresses the
# 4-bytes-later prologue the scan would otherwise report.
#
# RANGES is keyed by PROGRAM label, not by address: every slot-B image loads at
# 0x801F69D8, so a bare address list would force-disassemble one image's entry
# inside another image's bytes and print convincing garbage.
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_summon_ozma_0934.bin -noanalysis \
#       -postScript /scripts/dump_static_overlay.py

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.program.model.address import AddressSet
from ghidra.program.model.symbol import RefType, SourceType
from ghidra.util.task import ConsoleTaskMonitor

# {program label: [(entry VA, end VA exclusive), ...]}.
# Program label = program name minus ".bin". File offsets are into the PROT
# entry (VA - base); the base is 0x801F69D8 for every row here.
RANGES = {
    "overlay_summon_gimard_0903": [("801f69d8", "801f7724")],
    "overlay_summon_stager_x83_0905": [("801f69d8", "801f8078"), ("801f8078", "801f81e4")],
    "overlay_summon_nighto_0907": [("801f69e8", "801f7fa8"), ("801f7fa8", "801f81d0")],
    "overlay_stager_ultimate_rave_0924": [("801f6a18", "801f7820"), ("801f7820", "801f787c")],
    "overlay_summon_juggernaut_0927": [("801f6a84", "801f85a8"), ("801f85a8", "801f8988")],
    "overlay_summon_palma_0928": [("801f69f4", "801f8e68"), ("801f8e68", "801f9208")],
    "overlay_summon_mule_0929": [("801f69fc", "801f8c30"), ("801f8c30", "801f90a4")],
    "overlay_summon_horn_0930": [("801f6a74", "801f7ea4"), ("801f7ea4", "801f857c")],
    "overlay_summon_jedo_0931": [("801f6a58", "801f8adc"), ("801f8adc", "801f8b68")],
    "overlay_summon_meta_0932": [("801f6a34", "801f84a4"), ("801f84a4", "801f84dc")],
    "overlay_summon_terra_0933": [("801f6a30", "801f8748"), ("801f8748", "801f881c")],
    "overlay_summon_ozma_0934": [("801f6a40", "801f92ac"), ("801f92ac", "801f9c08")],
    "overlay_summon_effect_table_0957": [("801f6a14", "801f798c"), ("801f798c", "801f99f4"),
                                         ("801f99f4", "801f9ba8"), ("801f9ba8", "801f9c20")],
    # The rest of the module band (PROT 0903..0966 minus the rows above and
    # minus 0915 / 0926 / 0935, which carry no map row - see
    # crates/asset/data/static-overlays.toml). Same recovery, mechanised:
    # a function runs from its `addiu sp, sp, -F` prologue to the first
    # `jr ra` whose delay slot restores the SAME F, and the two entry VAs
    # PROT 0898 holds for the module (0x801CF4EC arm, 0x801F6734 row) are
    # added to that start set. Frame matching is what makes the partition
    # exact where the prologue and `jr ra` counts do NOT interleave - a
    # frameless leaf, an early `jr ra`, or a `jr ra` word sitting in the
    # data tail all break the count rule and none of them breaks this one.
    # A table entry that opens with a branch keeps its prologue in the delay
    # slot (PROT 0922's stager at 0x801F90E4), so the table VA wins over the
    # 4-bytes-later prologue.
    #
    # Four ranges below start EARLIER than their prologue on purpose. A function
    # here may open with a preamble that materialises a global before the frame
    # is set up, so its real entry is up to four instructions above the
    # `addiu sp, sp, -X` a prologue scan finds: 0904 at 0x801F8B84 (prologue
    # 0x801F8B8C), 0910 at 0x801F7DD0 (0x801F7DD8), and 0946 / 0953 at
    # 0x801F69FC (0x801F6A0C - a `lui`/`lw` of the battle ctx 0x8007BD24 plus a
    # `lbu` of the frame-delta scalar 0x1F800393). A CALLER found each of them:
    # 0904 / 0910 through their own image's internal `jal`, which
    # `port-catalog.py --missing-dumps` reports as a cited address with no dump;
    # 0946 / 0953 through PROT 0898's capture-class arm table 0x801CF56C.
    "overlay_summon_theeder_0904": [
        ("801f69d8", "801f815c"), ("801f815c", "801f83a4"), ("801f83a4", "801f8634"),
        ("801f8634", "801f8b84"), ("801f8b84", "801f8eac"), ("801f8eac", "801f8ee4")
    ],  # 6 fn, 9476/12288 B code
    "overlay_summon_gizam_0906": [
        ("801f69f4", "801f7740"), ("801f7740", "801f79e4")
    ],  # 2 fn, 4080/6144 B code
    "overlay_summon_zenoir_0908": [
        ("801f69d8", "801f8310"), ("801f8310", "801f8524"), ("801f8eac", "801f8ee4")
    ],  # 3 fn, 7044/10240 B code
    "overlay_summon_viguro_0909": [
        ("801f69f4", "801f7948"), ("801f7948", "801f7af4"), ("801f7af4", "801f7cc8"),
        ("801f7cc8", "801f7d30"), ("801f7d30", "801f7e18")
    ],  # 5 fn, 5156/8192 B code
    "overlay_summon_swordie_0910": [
        ("801f69ec", "801f7c18"), ("801f7c18", "801f7dd0"), ("801f7dd0", "801f81dc"),
        ("801f81dc", "801f89d4"), ("801f89d4", "801f8a80"), ("801f8eac", "801f8ee4")
    ],  # 6 fn, 8388/10240 B code
    "overlay_summon_orb_0911": [
        ("801f69d8", "801f7fe8"), ("801f7fe8", "801f804c")
    ],  # 2 fn, 5748/8192 B code
    "overlay_summon_freed_0912": [
        ("801f69d8", "801f835c"), ("801f835c", "801f852c")
    ],  # 2 fn, 6996/12288 B code
    "overlay_summon_nova_0913": [
        ("801f69f0", "801f864c"), ("801f864c", "801f8790")
    ],  # 2 fn, 7584/10240 B code
    "overlay_summon_gola_gola_0914": [
        ("801f69f0", "801f7a80"), ("801f7a80", "801f7ba8")
    ],  # 2 fn, 4536/6144 B code
    "overlay_summon_aluru_0916": [
        ("801f69f8", "801f88f8"), ("801f88f8", "801f8b3c")
    ],  # 2 fn, 8516/10240 B code
    "overlay_summon_barra_0917": [
        ("801f6a30", "801f82d8"), ("801f82d8", "801f85b8")
    ],  # 2 fn, 7048/12288 B code
    "overlay_summon_kemaro_0918": [
        ("801f6c70", "801f8ab0"), ("801f8ab0", "801f8b88")
    ],  # 2 fn, 7960/12288 B code
    "overlay_summon_spoon_0919": [
        ("801f69d8", "801f8578"), ("801f8578", "801f866c")
    ],  # 2 fn, 7316/10240 B code
    "overlay_summon_slippery_0920": [
        ("801f69d8", "801f7b88"), ("801f7b88", "801f81e8"), ("801f81e8", "801f81f0"),
        ("801f8578", "801f866c")
    ],  # 4 fn, 6412/8192 B code
    "overlay_summon_iota_0921": [
        ("801f6a08", "801f800c"), ("801f800c", "801f8120")
    ],  # 2 fn, 5912/10240 B code
    "overlay_summon_puera_0922": [
        ("801f6a3c", "801f90e4"), ("801f90e4", "801f9104")
    ],  # 2 fn, 9928/14336 B code
    "overlay_summon_gilium_0923": [
        ("801f69d8", "801f8b90"), ("801f8b90", "801f8cb4")
    ],  # 2 fn, 8924/16384 B code
    "overlay_summon_spikefish_0925": [
        ("801f6a00", "801f7ae8"), ("801f7ae8", "801f7d3c")
    ],  # 2 fn, 4924/6144 B code
    "overlay_cast_hyper_crush_0936": [
        ("801f69d8", "801f7bd0"), ("801f7bd0", "801f7ccc")
    ],  # 2 fn, 4852/6144 B code
    "overlay_cast_hyper_lightning_0937": [
        ("801f69d8", "801f7850"), ("801f7850", "801f79dc")
    ],  # 2 fn, 4100/6144 B code
    "overlay_cast_chaos_breath_0938": [
        ("801f69ec", "801f726c"), ("801f726c", "801f7a40"), ("801f7a40", "801f7ab8"),
        ("801f7ab8", "801f7bc8")
    ],  # 4 fn, 4572/6144 B code
    "overlay_cast_spore_gas_0939": [
        ("801f69d8", "801f74b4"), ("801f74b4", "801f74bc")
    ],  # 2 fn, 2788/6144 B code
    "overlay_cast_glare_divide_0940": [
        ("801f69f8", "801f7240"), ("801f7240", "801f78b8"), ("801f78b8", "801f8228"),
        ("801f8228", "801f82cc"), ("801f82cc", "801f82d4")
    ],  # 5 fn, 6364/8192 B code
    "overlay_cast_steal_0941": [
        ("801f6a04", "801f730c"), ("801f730c", "801f7d38"), ("801f7d38", "801f7db0"),
        ("801f7db0", "801f7e70")
    ],  # 4 fn, 5228/8192 B code
    "overlay_cast_power_up_0942": [
        ("801f69f4", "801f7d34"), ("801f7d34", "801f80a0"), ("801f80a0", "801f8118"),
        ("801f8118", "801f81b8")
    ],  # 4 fn, 6084/8192 B code
    "overlay_cast_curse_0943": [
        ("801f6a04", "801f6ef4"), ("801f6ef4", "801f7624"), ("801f7624", "801f769c"),
        ("801f769c", "801f7770"), ("801f7d34", "801f80a0"), ("801f80a0", "801f8118"),
        ("801f8118", "801f81b8")
    ],  # 7 fn, 4592/6144 B code
    "overlay_cast_guilty_cross_0944": [
        ("801f6a04", "801f7470"), ("801f7470", "801f7ebc"), ("801f7ebc", "801f7f2c"),
        ("801f7f2c", "801f7fd0")
    ],  # 4 fn, 5580/8192 B code
    "overlay_cast_water_column_0945": [
        ("801f69f8", "801f6edc"), ("801f6edc", "801f76f4"), ("801f76f4", "801f776c"),
        ("801f776c", "801f7810"), ("801f7ebc", "801f7f2c"), ("801f7f2c", "801f7fd0")
    ],  # 6 fn, 3884/6144 B code
    "overlay_cast_call_wave_0946": [
        ("801f69fc", "801f76c4"), ("801f76c4", "801f78cc")
    ],  # 2 fn, 3776/6144 B code
    "overlay_cast_v_windhash_0947": [
        ("801f69f0", "801f78f8"), ("801f78f8", "801f7900")
    ],  # 2 fn, 3856/6144 B code
    "overlay_cast_cross_beam_0948": [
        ("801f69f0", "801f726c"), ("801f726c", "801f8504"), ("801f8504", "801f8564")
    ],  # 3 fn, 7028/8192 B code
    "overlay_cast_water_crystals_0949": [
        ("801f6a10", "801f75bc"), ("801f75bc", "801f7630"), ("801f8504", "801f8564")
    ],  # 3 fn, 3200/8192 B code
    "overlay_cast_rolling_flare_0950": [
        ("801f6a24", "801f79f8"), ("801f79f8", "801f8190"), ("801f8190", "801f8208"),
        ("801f8208", "801f8240")
    ],  # 4 fn, 6172/8192 B code
    "overlay_cast_chaos_flare_0951": [
        ("801f6a20", "801f77e8"), ("801f77e8", "801f816c"), ("801f816c", "801f81dc"),
        ("801f81dc", "801f82ec")
    ],  # 4 fn, 6348/10240 B code
    "overlay_cast_bloody_horns_0952": [
        ("801f6a0c", "801f7118"), ("801f7118", "801f7b28"), ("801f7b28", "801f7ba0"),
        ("801f7ba0", "801f7ba8")
    ],  # 4 fn, 4508/6144 B code
    "overlay_cast_terio_punch_0953": [
        ("801f69fc", "801f7624"), ("801f7624", "801f77c8")
    ],  # 2 fn, 3516/6144 B code
    "overlay_cast_fatal_decision_0954": [
        ("801f6a58", "801f85d4"), ("801f85d4", "801f85dc")
    ],  # 2 fn, 7044/10240 B code
    "overlay_cast_white_shield_0955": [
        ("801f6a28", "801f7158"), ("801f7158", "801f767c"), ("801f767c", "801f7fa4"),
        ("801f7fa4", "801f86a4"), ("801f86a4", "801f8f0c"), ("801f8f0c", "801f92a4"),
        ("801f92a4", "801f9370"), ("801f9370", "801f93d4")
    ],  # 8 fn, 10668/14336 B code
    "overlay_cast_water_hazard_0956": [
        ("801f69d8", "801f7298"), ("801f7298", "801f7e4c"), ("801f7e4c", "801f7ec4"),
        ("801f7ec4", "801f7fc8")
    ],  # 4 fn, 5616/8192 B code
    "overlay_cast_blazing_slash_0958": [
        ("801f6dd8", "801f8d30"), ("801f8d30", "801f8e60"), ("801f8e60", "801f8eb8")
    ],  # 3 fn, 8416/12288 B code
    "overlay_cast_megaton_press_0959": [
        ("801f69f0", "801f8250"), ("801f8250", "801f87f4"), ("801f87f4", "801f884c")
    ],  # 3 fn, 7772/12288 B code
    "overlay_cast_plasma_strike_0960": [
        ("801f69d8", "801f74e4"), ("801f74e4", "801f8638"), ("801f8638", "801f86b0"),
        ("801f86b0", "801f8768")
    ],  # 4 fn, 7568/10240 B code
    "overlay_cast_dead_end_crisis_0961": [
        ("801f69d8", "801f78a4"), ("801f78a4", "801f7a54"), ("801f7a54", "801f7ab4"),
        ("801f8638", "801f86b0"), ("801f86b0", "801f8768")
    ],  # 5 fn, 4620/8192 B code
    "overlay_cast_blade_breath_0962": [
        ("801f69d8", "801f6d54"), ("801f6d54", "801f74a0"), ("801f74a0", "801f7ae4"),
        ("801f7ae4", "801f8080"), ("801f8080", "801f813c"), ("801f813c", "801f81e8")
    ],  # 6 fn, 6160/10240 B code
    "overlay_cast_genocidal_cannon_0963": [
        ("801f6a20", "801f81a0"), ("801f81a0", "801f8438"), ("801f8438", "801f8490")
    ],  # 3 fn, 6768/12288 B code
    "overlay_cast_element_change_0964": [
        ("801f69d8", "801f88ec"), ("801f88ec", "801f8bf8"), ("801f8bf8", "801f8e3c"),
        ("801f8e3c", "801f8eb8"), ("801f9ba8", "801f9c20")
    ],  # 5 fn, 9560/14336 B code
    "overlay_cast_doomsday_0965": [
        ("801f69d8", "801f7b1c"), ("801f7b1c", "801f7b74"), ("801f7b74", "801f7c2c")
    ],  # 3 fn, 4692/8192 B code
    "overlay_cast_evil_seru_magic_0966": [
        ("801f6a74", "801f8d64"), ("801f8d64", "801f9160")
    ],  # 2 fn, 9964/16384 B code
    # Ordinary overlays whose routine the walk below cannot bound, because the
    # coverage run it starts from is interior: the prologue is behind the run's
    # start and the `jr ra` is past its end. Bounds recovered the same way -
    # nearest preceding `addiu sp, sp, -X`, first following `jr ra` + delay.
    "overlay_dance_0980": [("801cef54", "801cf470"), ("801d32f8", "801d387c")],
    # PROT 0978 holds exactly one prologue and one `jr ra`: one function over
    # the whole code region. The pre-existing 0x801F6B24 dump prints the same
    # span but reports a 328-byte body, so the coverage credit is short.
    "overlay_field_back_read_0978": [("801f6b24", "801f7358")],
    # The rest of the byte-derived worklist, outside the module band. These
    # images have their own call graphs, but the runs the worklist reports are
    # bodies Ghidra's analysis either never carved or carved short - the two
    # re-dumps below (0975, 0979) each replace a dump that printed at the right
    # entry with a fraction of the real body (3060 of 3864, 316 of 2276).
    #
    # Recovering them needed one more epilogue idiom than the module band did:
    # the sp restore is not always in the `jr ra` delay slot. In PROT 0902 /
    # 0973 / 0974 the compiler emits `addiu sp, sp, +F` BEFORE the `jr ra`,
    # with something else in the delay slot, and a matcher that only looks at
    # the delay slot reports every one of those functions as unterminated.
    "overlay_gameover_0902": [("801ceb50", "801cec44")],
    "overlay_fishing_0972": [("801cf070", "801cf3bc")],
    "overlay_other2_dev_0973": [("801ce8a0", "801ce8ec"), ("801ce8ec", "801ceb58"),
                                ("801ceb58", "801cefd0")],
    "overlay_other3_dev_0974": [("801ce85c", "801ced68"), ("801ced68", "801cee80"),
                                ("801cee80", "801cef24")],
    "overlay_slot_machine_0975": [("801cf0d8", "801cfff0")],
    "overlay_field_battle_intro_0979": [("801ce8cc", "801cf1b0")],
    "overlay_battle_tutorial_0967": [("801f747c", "801f7628")],
    # PROT 0901's middle band is a SHARED-TAIL leaf family, not a sequence of
    # ordinary functions: 0x801F7644..0x801F8EB4 carries frameless draw leaves
    # with six `jal`s and NOT ONE `jr ra` - each leaf `j`s to the common exit at
    # 0x801F8EB4. Neither a prologue partition nor `walk_range`'s cut-at-`jr ra`
    # can split that, so the family is dumped as one range up to the next real
    # prologue. Treat the dump as the family, not as one routine.
    "overlay_world_map_render_0901": [("801f7644", "801f8f28")],
    # SCUS: a two-instruction `jr ra; nop` null leaf that PROT 0895's
    # FUN_801CEFD4 calls. Nothing else in the corpus dumps it, and a cited
    # address with no dump is exactly what `port-catalog.py --missing-dumps`
    # reports. Program name `SCUS_942.54` -> label `SCUS_942_54`; the dump
    # lands at `funcs/8003f120.txt` (no overlay prefix), which is what
    # `out_path_for` does for a SCUS program.
    "SCUS_942_54": [("8003f120", "8003f128")],
    # The two slot-B cast-tick dispatchers, dumped from the BASED battle-action
    # image so the module band's entry tables have a provenance that names its
    # own image. The pre-existing dumps of these VAs are `overlay_muscle_dome_*`
    # and `overlay_0897_*`; the first is the same bytes under a capture-derived
    # name, the second is a VA collision with entirely different functions.
    "overlay_battle_action_0898": [("801f1ed4", "801f2160"), ("801f2160", "801f2410")],
}

# {program label: [(start VA, end VA exclusive), ...]} for images that DO have
# an internal call graph, where Ghidra's analysis already found the functions
# and the gap is only that nothing dumped them. Each range is walked: every
# function entry inside it is dumped, and any run of bytes with no function is
# force-disassembled and split at each `jr ra` + delay-slot pair, the shape a
# sequence of separately-emitted leaves has. Ranges come from
# `scripts/ci/disc-coverage.py`'s un-dumped `code` runs.
WALK_RANGES = {
    "overlay_field_0897": [
        ("801da930", "801daa50"), ("801dd4c4", "801dd9d4"), ("801e5154", "801e5338"),
        ("801f0718", "801f0adc"), ("801f0efc", "801f1138"), ("801f23b4", "801f26b4"),
        ("801f2db4", "801f30c4"), ("801f30d4", "801f32d4"),
    ],
    "overlay_dance_0980": [("801d05b8", "801d0750")],
    "overlay_arena_init_0977": [("801cf20c", "801cf870"), ("801d0cd0", "801d0e78")],
    # PROT 0895 (`init.pak`) is a slot-A image with a real internal call graph -
    # 23 internal `jal`s, 8 of them landing on prologues, which is what recovers
    # its base. Its whole code region is one run: file +0x1A8 .. +0x216C, i.e.
    # from the mode-16 entry `FUN_801CE9C0` to the last frame-matched `jr ra`.
    # Everything above that is the logo TIM payload (first TIM at file 0x21C4).
    "overlay_boot_init_pak_0895": [("801ce9c0", "801d0984")],
}

# Runs `disc-coverage.py` ranks as `code` that the bytes say are DATA, so no
# dump belongs there. Kept as a list rather than deleted, because the next
# reader of the worklist will otherwise re-walk them. Each was read with
# `scripts/ghidra-analysis/disasm-overlay-fn.py` at the image's own base:
# they decode as `.byte` runs, `nop` fields and impossible operands
# (`j 0x80300000`, `tge`, `syscall`), never as a body reaching a `jr ra`.
#
#   field(897)            0x801F23B4, 0x801F2DB4, 0x801F30D4
#   dance(980)            0x801D43A4, 0x801D4AA4
#   arena_init(977)       0x801D1EF0                (image tail)
#   field_back_read(978)  0x801F7624                (image tail)
#   menu(899)             0x801E4530, 0x801E4C30, 0x801E5030  (data segment -
#                         short little-endian records with a zero high half,
#                         the menu overlay's own tables; see field-menu.md)
#   gameover(902)         0x801CED48, 0x801CEF6C    (past the last function)
#   fishing(972)          0x801D7E30                (image tail)
#   other3_dev(974)       0x801D1878                (nop field + `.byte` runs)
#   SCUS_942.54           0x80074F80, 0x80075380, 0x80076880, 0x80076E80,
#                         0x80077480, 0x80078B80, 0x8007A880  (static tables)
#
# SCUS 0x80045CB4 is the one SCUS run that IS code, and it is still not a dump
# target: nothing in any image references it (five-form scan), its preceding
# word is a `sw` in the same instruction stream, and the nearest prologue is
# 11128 bytes back - the INTERIOR class of docs/tooling/worklist-classification.md.
NOT_CODE = ()

# {program label: (dispatcher `jr` VA, jump-table VA, arm count)}. Teaching
# Ghidra the computed jump keeps the decompiler's per-arm structure; without it
# the arms decompile as unreachable code. The arm count is the module's own
# `sltiu` bound, read out of the dispatcher - not a guess about table length.
JUMPTABLES = {
    "overlay_summon_ozma_0934": ("801f6ad0", "801f69d8", 0x1a),
}

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
label = prog_name.replace(".bin", "").replace(".", "_")
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
refs = prog.getReferenceManager()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return addr is not None and mem.getBlock(addr) is not None


def dump_function(func):
    addr_str = "%08x" % func.getEntryPoint().getOffset()
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 180, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    finally:
        fh.close()
    return out_path


def add_jumptable():
    spec = JUMPTABLES.get(label)
    if spec is None:
        return
    jr_addr = af.getAddress(spec[0])
    tbl = af.getAddress(spec[1])
    if not in_program(jr_addr) or not in_program(tbl):
        return
    added = 0
    for i in range(spec[2]):
        word = mem.getInt(tbl.add(i * 4)) & 0xFFFFFFFF
        tgt = af.getAddress("%08x" % word)
        if not in_program(tgt):
            continue
        refs.addMemoryReference(jr_addr, tgt, RefType.COMPUTED_JUMP,
                                SourceType.USER_DEFINED, 0)
        added += 1
    print("  jumptable {}: {} arms".format(spec[1], added))


def cover_range(start_str, end_str):
    start = af.getAddress(start_str)
    end = af.getAddress(end_str)
    if not in_program(start) or not in_program(end.subtract(1)):
        return None
    span = AddressSet(start, end.subtract(1))

    # Interior function entries would block setBody; the range partition is the
    # authority on where a body starts, so drop anything inside it but the head.
    for func in list(fm.getFunctions(span, True)):
        if func.getEntryPoint().compareTo(start) != 0:
            fm.removeFunction(func.getEntryPoint())

    listing.clearCodeUnits(start, end.subtract(1), False)
    cursor = start
    while cursor.compareTo(end) < 0:
        if listing.getInstructionAt(cursor) is None:
            DisassembleCommand(cursor, span, True).applyTo(prog, monitor)
        ins = listing.getInstructionAt(cursor)
        cursor = cursor.add(ins.getLength() if ins is not None else 4)

    func = fm.getFunctionAt(start)
    if func is None:
        CreateFunctionCmd(start).applyTo(prog, monitor)
        func = fm.getFunctionAt(start)
    if func is None:
        print("  [warn] no function created at {}".format(start_str))
        return None
    try:
        func.setBody(span)
    except Exception as e:
        print("  [warn] setBody {}..{}: {}".format(start_str, end_str, e))
    return func


def walk_range(start_str, end_str):
    """Dump every function inside a range; force the runs that hold none."""
    start = af.getAddress(start_str)
    end = af.getAddress(end_str)
    if not in_program(start) or not in_program(end.subtract(1)):
        return
    span = AddressSet(start, end.subtract(1))
    print("=== walk {}..{}".format(start_str, end_str))

    cursor = start
    while cursor.compareTo(end) < 0:
        func = fm.getFunctionContaining(cursor)
        if func is not None:
            print("  {} size={} -> {}".format(
                func.getEntryPoint(), func.getBody().getNumAddresses(),
                dump_function(func)))
            nxt = func.getBody().getMaxAddress().add(1)
            cursor = nxt if nxt.compareTo(cursor) > 0 else cursor.add(4)
            continue
        # No function here: force-disassemble and cut at the first `jr ra`.
        if listing.getInstructionAt(cursor) is None:
            DisassembleCommand(cursor, span, True).applyTo(prog, monitor)
        probe, jr_at = cursor, None
        while probe.compareTo(end) < 0:
            ins = listing.getInstructionAt(probe)
            if ins is None:
                break
            if ins.getMnemonicString().lower() == "jr" and "ra" in ins.toString():
                jr_at = probe
                break
            probe = probe.add(ins.getLength())
        if jr_at is None:
            print("  no `jr ra` from {} - stopping this range".format(cursor))
            return
        CreateFunctionCmd(cursor).applyTo(prog, monitor)
        made = fm.getFunctionAt(cursor)
        if made is None:
            print("  [warn] no function created at {}".format(cursor))
            cursor = jr_at.add(8)
            continue
        print("  {} size={} -> {} (forced)".format(
            cursor, made.getBody().getNumAddresses(), dump_function(made)))
        nxt = made.getBody().getMaxAddress().add(1)
        cursor = nxt if nxt.compareTo(cursor) > 0 else jr_at.add(8)


for rng in RANGES.get(label, []):
    func = cover_range(rng[0], rng[1])
    if func is None:
        continue
    add_jumptable()
    print("  {} size={} -> {}".format(
        rng[0], func.getBody().getNumAddresses(), dump_function(func)))

for rng in WALK_RANGES.get(label, []):
    walk_range(rng[0], rng[1])

print("done [{}]".format(prog_name))
