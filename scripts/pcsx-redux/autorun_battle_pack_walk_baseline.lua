-- autorun_battle_pack_walk_baseline.lua
--
-- Retail baseline for the two candidate producers of the rebuilt-PROT-0874
-- wild read (`docs/formats/character-mesh.md`, open thread "What breaks a
-- rebuilt PROT 0874 container"). Both live in the battle loader
-- `FUN_800520F0` and differ only in which buffer they walk:
--
--   byte-offset walk  0x8005255C..0x8005259C   base `*0x8007B878`
--                     addr = base + word          consumer FUN_8001FBCC
--   word-offset walk  0x800525A0..0x80052600   base `*(gp+0xA8C)` (the arena)
--                     addr = base + (word << 2)   consumer FUN_80026B4C
--
-- Both walks form the pointer in the `jal`'s DELAY SLOT (`addu a0, s2, a0` at
-- `0x80052588`, `addu a0, v1, a0` at `0x800525DC`), so `a0` read AT the `jal`
-- is the offset operand and the base is still in its own register. This probe
-- breakpoints the two `jal` sites and reconstructs `base + operand`, which
-- keeps the breakpoints inside the walks: the two callees are shared routines
-- with many other callers, and a breakpoint on either would fire thousands of
-- times a run for rows this measurement does not want.
--
-- It also write-watches `0x8007B878` so the three writers (`0x8001F268` in the
-- sub-asset install dispatcher's type-2 arm, `0x8005250C` and `0x80052538`,
-- both `arena + streamed bytes`) are separated by which one actually fires on
-- a battle load, and it records `gp[+0xA8C]` and each buffer's count word.
--
-- On a RETAIL disc this is the reference run. On a disc whose PROT 0874 §0
-- decodes to a different size, the same run says which walk leaves the RAM
-- window - that is the deciding observation the thread is waiting on, and it
-- has to come from a COLD BOOT because a save state replays the RAM of the
-- disc that booted it.
--
-- Outputs (probe.out_path):
--   walks.csv    one row per walk iteration: which walk, iteration index, the
--                computed address, the base, the offset word and `ra`.
--   writers.csv  one row per write to 0x8007B878: pc, ra, pre-value.
--   bases.csv    per-vsync sample of 0x8007B878, gp[+0xA8C] and both counts,
--                logged on change.
--
-- Env: LEGAIA_SSTATE (a FIELD state that enters a battle on its own is the
-- right shape), LEGAIA_FRAMES (default 3000), LEGAIA_MAX_ROWS (default 4000),
-- LEGAIA_LABEL.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 3000)
local MAX_ROWS = probe.getenv_num("LEGAIA_MAX_ROWS", 4000)
local LABEL    = probe.getenv("LEGAIA_LABEL", "pack-walk")

local B878         = 0x8007B878
local GP_ARENA_OFF = 0xA8C
local BYTE_JAL = 0x80052584   -- jal FUN_8001FBCC; a0 = offset word, s2 = base
local WORD_JAL = 0x800525D8   -- jal FUN_80026B4C; a0 = word << 2, v1 = base
local LOADER       = 0x800520F0
local MODE_VA      = 0x8007B83C

local function u32(a) return probe.read_u32(a) or 0 end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function regs() local r = PCSX.getRegisters() return (r.GPR and r.GPR.n) or {} end

local walks_csv, writers_csv, bases_csv
local n_byte, n_word, n_writes, loader_hits = 0, 0, 0, 0
local rows = 0
local vsync = 0
local gp_base = 0
local last_bases = ""

probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== battle pack-walk baseline == label=%s", LABEL))
        probe.env.write_manifest("autorun_battle_pack_walk_baseline.lua",
            { label = LABEL, sstate = SSTATE, frames = FRAMES })
        walks_csv = probe.csv_open(probe.out_path("walks.csv"),
            "vsync,walk,iter,addr,base,offset_word,ra,in_ram")
        writers_csv = probe.csv_open(probe.out_path("writers.csv"),
            "vsync,pc,ra,prev_value,arena")
        bases_csv = probe.csv_open(probe.out_path("bases.csv"),
            "vsync,mode,b878,arena,b878_count,arena_count")

        local function walk_site(addr, name)
            probe.arm_breakpoint(addr, "Exec", 4, name, function()
                local n = regs()
                local operand = tou32(n.a0)
                local base = (name == "byte") and tou32(n.s2) or tou32(n.v1)
                local ptr = bit.band(base + operand, 0xFFFFFFFF)
                local iter
                if name == "byte" then
                    n_byte = n_byte + 1
                    iter = n_byte
                else
                    n_word = n_word + 1
                    iter = n_word
                end
                if rows >= MAX_ROWS then return end
                rows = rows + 1
                walks_csv:row("%d,%s,%d,0x%08X,0x%08X,0x%08X,0x%08X,%d",
                    vsync, name, iter, ptr, base, operand, tou32(n.ra),
                    probe.in_ram(ptr, 4) and 1 or 0)
            end)
        end
        walk_site(BYTE_JAL, "byte")
        walk_site(WORD_JAL, "word")

        probe.arm_breakpoint(LOADER, "Exec", 4, "loader", function()
            loader_hits = loader_hits + 1
        end)

        probe.arm_breakpoint(B878, "Write", 4, "b878w", function()
            local n = regs()
            local r = PCSX.getRegisters()
            n_writes = n_writes + 1
            -- The debug hook runs BEFORE the store, so this is the PRE value.
            writers_csv:row("%d,0x%08X,0x%08X,0x%08X,0x%08X", vsync,
                tou32(r.pc), tou32(n.ra), u32(B878),
                gp_base ~= 0 and u32(gp_base + GP_ARENA_OFF) or 0)
        end)
        return {}
    end,

    on_capture = function(_c, elapsed)
        vsync = elapsed
        if gp_base == 0 then
            local g = tou32(regs().gp)
            if g >= 0x80070000 and g < 0x80080000 then gp_base = g end
        end
        if gp_base == 0 then return end
        local b = u32(B878)
        local arena = u32(gp_base + GP_ARENA_OFF)
        local line = string.format("%08X/%08X", b, arena)
        if line ~= last_bases then
            last_bases = line
            bases_csv:row("%d,%d,0x%08X,0x%08X,%d,%d", elapsed,
                probe.read_u8(MODE_VA) or 0, b, arena,
                probe.in_ram(b, 4) and u32(b) or -1,
                probe.in_ram(arena, 4) and u32(arena) or -1)
        end
    end,

    on_done = function()
        PCSX.log(string.format(
            "[pack-walk] loader=%d byte_walk=%d word_walk=%d b878_writes=%d gp=0x%08X",
            loader_hits, n_byte, n_word, n_writes, gp_base))
        if walks_csv then walks_csv:close() end
        if writers_csv then writers_csv:close() end
        if bases_csv then bases_csv:close() end
    end,
})
