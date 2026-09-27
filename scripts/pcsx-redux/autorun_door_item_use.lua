-- autorun_door_item_use.lua
--
-- Retail capture of a Door of Light (item 0x88) / Door of Wind (0x89) use,
-- from the pause menu to the destination scene's load. The question it
-- answers: what is the per-vsync timeline of the field overlay's pause-menu
-- session FUN_801ED308 (handler 0x30) after the menu closes, the travel art it
-- hands the actor to (0x29 Riremito FUN_801EE094 / 0x2B Rula FUN_801EE328),
-- and the fade the art spawns - the shape World::tick_pause_session models
-- (docs/subsystems/field-locomotion.md, docs/subsystems/world-map.md).
--
-- Setup: at each "vsync:id" of LEGAIA_DOOR_POKES (default "12:0x88"), writes
-- that item x1 into the first slot of the bag's active window
-- (gp[+0x2D2]..gp[+0x2D4], bag 0x80085958, [id][count] slots;
-- docs/subsystems/inventory.md), so it is row 0 of the Use list. Then a pad
-- script opens the menu, Items, Use, picks row 0 and answers the confirm.
-- For the Door of Wind the "confirm" press picks destination row 0.
--
-- Observation:
--   * Exec BP 0x801F1634 - the subsystem actor dispatcher's `jalr` on the
--     handler table (FUN_801F159C in PROT 0897; s0 = the actor). Guarded on
--     the instruction word so the menu overlay's bytes at the same VA do not
--     count. Logs handler id +0x50, phase +0x54, dwell +0x9E per dispatch.
--   * Exec BP 0x80024E80 (SCUS fade spawn FUN_80024E80): template words, id.
--   * Exec BP 0x8001FD44 (SCUS scene-name copy): the destination name.
--   * Per vsync: game mode 0x8007B83C, exit code _DAT_8007B43C, brightness
--     _DAT_8007B440, pad hold _DAT_8007B6B4, frame step 0x1F800393, the
--     Door gate bits 0x1F800394, the world-map return triple
--     0x80084628 @ (0x80084624, 0x8008462C), the arrival seat
--     0x80073EF4 / 0x80073EF8 the resolve writes, the last dispatched actor's +0x50/+0x54/+0x9E,
--     the player actor's (0x8007C364) +0x14..+0x1C, and the scene name
--     0x80084548. A row is written only when one of them changes.
--
-- Output: <LEGAIA_OUT_DIR>/door.log.
-- Env: LEGAIA_SSTATE / LEGAIA_FRAMES / LEGAIA_OUT_DIR (run_probe.sh),
--      LEGAIA_DOOR_POKES ("vsync:0x88,vsync:0x89"), LEGAIA_DOOR_SCRIPT (comma-separated
--      "vsync:BUTTON" presses; default below).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")
local bp = require("probe.bp")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1500)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/door_item_use")
local POKES = probe.getenv("LEGAIA_DOOR_POKES", "12:0x88")
local SCRIPT = probe.getenv("LEGAIA_DOOR_SCRIPT",
    "60:SELECT,200:CROSS,320:CROSS,440:CROSS,560:CROSS")
os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/door.log", "w")
local function log(s)
    PCSX.log("[door] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local function u32(x) return (tonumber(x) or 0) % 0x100000000 end
local function s16(v) v = v or 0; if v >= 0x8000 then return v - 0x10000 end; return v end

local GP = 0x8007B318
local BAG = 0x80085958
local JALR_WORD = 0x0040F809 -- jalr v0 at 0x801F1634

local vs = 0
local actor = nil

local presses = {}
for at, name in string.gmatch(SCRIPT, "(%d+):(%u+)") do
    presses[#presses + 1] = { at = tonumber(at), btn = pad.BTN[name], name = name }
end
local HOLD = 8

local function scene_name()
    local out = {}
    for i = 0, 11 do
        local c = probe.read_u8(0x80084548 + i) or 0
        if c == 0 then break end
        out[#out + 1] = string.char(c)
    end
    return table.concat(out)
end

local function poke_item(ITEM)
    local lo = s16(probe.read_u16(GP + 0x2D2))
    local hi = s16(probe.read_u16(GP + 0x2D4))
    local old_id = probe.read_u8(BAG + lo * 2) or 0
    local old_n = probe.read_u8(BAG + lo * 2 + 1) or 0
    probe.write_u8(BAG + lo * 2, ITEM)
    probe.write_u8(BAG + lo * 2 + 1, 1)
    log(string.format("poked item 0x%02X x1 into bag slot %d (was %02X x%d); window [%d,%d)",
        ITEM, lo, old_id, old_n, lo, hi))
end

local last_row = nil
local function sample()
    local mode = probe.read_u8(0x8007B83C) or 0
    local code = probe.read_u32(0x8007B43C) or 0
    local level = probe.read_u32(0x8007B440) or 0
    local hold = probe.read_u32(0x8007B6B4) or 0
    local step = probe.read_scratch_u8(0x1F800393) or 0
    local gate = probe.read_scratch_u32(0x1F800394) or 0
    local tx = probe.read_u32(0x80084624) or 0
    local map = probe.read_u32(0x80084628) or 0
    local tz = probe.read_u32(0x8008462C) or 0
    local ax = probe.read_u32(0x80073EF4) or 0
    local az = probe.read_u32(0x80073EF8) or 0
    local h, ph, dw = -1, -1, -1
    if actor ~= nil and mode == 3 then
        h = probe.read_u16(actor + 0x50) or -1
        ph = s16(probe.read_u16(actor + 0x54))
        dw = s16(probe.read_u16(actor + 0x9E))
    end
    local pl = probe.read_u32(0x8007C364) or 0
    local px, py, pz = 0, 0, 0
    if bit.tobit(bit.band(pl, 0xFF000000)) == bit.tobit(0x80000000) then
        px = s16(probe.read_u16(pl + 0x14))
        py = s16(probe.read_u16(pl + 0x18))
        pz = s16(probe.read_u16(pl + 0x1C))
    end
    local row = string.format(
        "mode=%02X code=%d level=%d hold=%08X step=%d gate=%08X h=%X ph=%d dw=%d p=(%d,%d,%d) ret=%X@(%d,%d) arr=(%d,%d) scene=%s",
        mode, code, level, u32(hold), step, u32(gate), h, ph, dw, px, py, pz,
        u32(map), u32(tx), u32(tz), u32(ax), u32(az), scene_name())
    if row ~= last_row then
        log(string.format("v=%d %s", vs, row))
        last_row = row
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        bp.arm(0x801F1634, "Exec", 4, "subsystem_dispatch", function()
            if (probe.read_u32(0x801F1634) or 0) ~= JALR_WORD then return end
            local r = PCSX.getRegisters()
            local s0 = u32(r.GPR.n.s0)
            if actor ~= s0 then
                log(string.format("v=%d dispatch actor %08X", vs, s0))
                actor = s0
            end
            log(string.format("v=%d  tick h=%X ph=%d dw=%d",
                vs, probe.read_u16(s0 + 0x50) or 0, s16(probe.read_u16(s0 + 0x54)),
                s16(probe.read_u16(s0 + 0x9E))))
        end)
        bp.arm(0x80024E80, "Exec", 4, "fade_spawn", function()
            local r = PCSX.getRegisters()
            local a0 = u32(r.GPR.n.a0)
            local w = {}
            for i = 0, 11 do w[#w + 1] = string.format("%04X", probe.read_u16(a0 + i * 2) or 0) end
            log(string.format("v=%d fade spawn a0=%08X a1=%d ra=%08X tmpl=[%s]",
                vs, a0, u32(r.GPR.n.a1), u32(r.GPR.n.ra), table.concat(w, " ")))
        end)
        bp.arm(0x8001FD44, "Exec", 4, "scene_load", function()
            local r = PCSX.getRegisters()
            log(string.format("v=%d FUN_8001FD44 a0=%08X a1=%08X ra=%08X",
                vs, u32(r.GPR.n.a0), u32(r.GPR.n.a1), u32(r.GPR.n.ra)))
        end)
        return {}
    end,

    on_capture = function(_ctx, tick)
        vs = tick
        for at, id in string.gmatch(POKES, "(%d+):(0x%x+)") do
            if tick == tonumber(at) then poke_item(tonumber(id)) end
        end
        for _, s in ipairs(presses) do
            if tick == s.at then
                pad.force(s.btn)
                log(string.format("v=%d press %s", tick, s.name))
            elseif tick == s.at + HOLD then
                pad.release(s.btn)
            end
        end
        sample()
    end,

    on_done = function()
        for _, s in ipairs(presses) do pad.release(s.btn) end
        log("=== door_item_use done ===")
        if LOG then LOG:close() end
    end,
})
