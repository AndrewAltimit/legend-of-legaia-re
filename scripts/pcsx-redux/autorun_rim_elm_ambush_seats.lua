-- autorun_rim_elm_ambush_seats.lua
--
-- Which seats does retail give the Rim Elm ambush's monsters, and does the
-- fight raise the Ra-Seru-forbidden bit 0x200 of _DAT_8007BAC0?
--
-- The formation roll's map-gated arm (first monster 0x3D..0x3F on map
-- _DAT_80084540 0x0C / 0x15) adds 4 to the monster seat row, and with the
-- scripted-fight bit (DAT_8007BD60 bit 7) also set the row lands on 9..12 of
-- 0x80077608, which the disc zero-fills (docs/subsystems/battle.md). The
-- rim_elm_queen_bee_battle state sits in town0c (map 0x15) a few seconds
-- before that fight starts on its own.
--
-- Observation:
--   * Exec BP FUN_800513F0 (battle init) and FUN_80051D84 (formation roll):
--     vsync, ra, and the words below at entry.
--   * Exec BP 0x80051850 (the seat loop's jal 0x80024C88; its delay slot adds
--     a0 + v0): the seat-row address a0 + v0, the row index v0 >> 5, the
--     map addend s4 and the scripted addend t2 (sp+0x20).
--   * Exec BP 0x801E0710 (PROT 0897, guarded on its word): the field VM's
--     op-0x3E scripted-battle arm about to install a formation; logs the
--     op's operand bytes at s6.
--   * Write BP on DAT_8007BD60: pc / ra / value, who raises the battle flags.
--   * Per vsync: game mode 0x8007B83C, map 0x80084540, special word
--     0x8007BAC0, DAT_8007BD60, the formation bytes 0x8007BD08..0x8007BD13,
--     and for pool slots 0..7 (0x801C9370 + i*4) the actor's +0x34 / +0x38
--     seat words. A row is written only when a field changes.
--
-- Output: <LEGAIA_OUT_DIR>/ambush.log.
-- Env: LEGAIA_SSTATE / LEGAIA_FRAMES / LEGAIA_OUT_DIR (run_probe.sh), plus
--   LEGAIA_HOLD         pad button held from the load (e.g. RIGHT, to walk
--                       into a random encounter from karisto_sol_pre_encounter)
--   LEGAIA_FIRST_MONSTER  hex id written to the formation's first byte
--                       0x8007BD0C at battle init's entry - a SYNTHETIC first
--                       monster, for the battle-init arm that tests it
--                       (0x800519DC..0x80051A04: 0xAF raises 0x200 in
--                       _DAT_8007BAC0). Say so wherever a result rests on it.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local bp = require("probe.bp")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/rim_elm_ambush_seats")
os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/ambush.log", "w")
local function log(s)
    PCSX.log("[ambush] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local function u32(x) return (tonumber(x) or 0) % 0x100000000 end
local function s32(x)
    x = u32(x)
    if x >= 0x80000000 then return x - 0x100000000 end
    return x
end

local vs = 0
local pad = require("probe.pad")
local HOLD = pad.BTN[probe.getenv("LEGAIA_HOLD", "")]
local FIRST = tonumber(probe.getenv("LEGAIA_FIRST_MONSTER", ""), 16)

local function words()
    return string.format("mode=%02X map=%X bac0=%08X bd60=%08X form=%s",
        probe.read_u8(0x8007B83C) or 0, u32(probe.read_u32(0x80084540)),
        u32(probe.read_u32(0x8007BAC0)), u32(probe.read_u32(0x8007BD60)),
        probe.bytes_to_hex(probe.read_bytes(0x8007BD08, 12)))
end

local function seats()
    local out = {}
    for i = 0, 7 do
        local a = u32(probe.read_u32(0x801C9370 + i * 4))
        if a >= 0x80000000 and a < 0x80200000 then
            out[#out + 1] = string.format("%d:(%d,%d)", i,
                s32(probe.read_u32(a + 0x34)), s32(probe.read_u32(a + 0x38)))
        else
            out[#out + 1] = string.format("%d:-", i)
        end
    end
    return table.concat(out, " ")
end

local last = nil
local function sample()
    local row = words() .. " seats " .. seats()
    if row ~= last then
        log(string.format("v=%d %s", vs, row))
        last = row
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        bp.arm(0x800513F0, "Exec", 4, "battle_init", function()
            local r = PCSX.getRegisters()
            if FIRST then
                probe.write_u8(0x8007BD0C, FIRST)
                log(string.format("v=%d SYNTHETIC first monster %02X", vs, FIRST))
            end
            log(string.format("v=%d FUN_800513F0 ra=%08X %s", vs, u32(r.GPR.n.ra), words()))
        end)
        bp.arm(0x80051D84, "Exec", 4, "formation_roll", function()
            local r = PCSX.getRegisters()
            log(string.format("v=%d FUN_80051D84 ra=%08X %s", vs, u32(r.GPR.n.ra), words()))
        end)
        bp.arm(0x80051850, "Exec", 4, "seat_row", function()
            local r = PCSX.getRegisters()
            local a0, v0 = u32(r.GPR.n.a0), u32(r.GPR.n.v0)
            local row = u32(a0 + v0)
            log(string.format("v=%d seat-row addr=%08X index=%d s4=%d t2=%d s2=%d bytes=%s",
                vs, row, math.floor(v0 / 32), s32(r.GPR.n.s4), s32(r.GPR.n.t2), s32(r.GPR.n.s2),
                probe.bytes_to_hex(probe.read_bytes(row, 8))))
        end)
        bp.arm(0x801E0710, "Exec", 4, "op3e_arm", function()
            if u32(probe.read_u32(0x801E0710)) ~= 0x0C076787 then return end
            local r = PCSX.getRegisters()
            local s6 = u32(r.GPR.n.s6)
            log(string.format("v=%d op-0x3E arm operands=%s", vs,
                probe.bytes_to_hex(probe.read_bytes(s6, 4))))
        end)
        bp.arm(0x8007BD60, "Write", 4, "bd60_write", function()
            local r = PCSX.getRegisters()
            log(string.format("v=%d BD60 write pc=%08X ra=%08X old=%08X", vs, u32(r.pc),
                u32(r.GPR.n.ra), u32(probe.read_u32(0x8007BD60))))
        end)
        return {}
    end,

    on_capture = function(_ctx, tick)
        vs = tick
        if HOLD then
            if (probe.read_u8(0x8007B83C) or 0) == 0x03 then pad.force(HOLD) else pad.release(HOLD) end
        end
        sample()
    end,

    on_done = function()
        log(string.format("v=%d final %s seats %s", vs, words(), seats()))
        log("=== rim_elm_ambush_seats done ===")
        if LOG then LOG:close() end
    end,
})
