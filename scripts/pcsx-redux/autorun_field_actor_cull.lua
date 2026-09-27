-- autorun_field_actor_cull.lua
--
-- Retail oracle for the field actor visibility cull FUN_801D79E8 (PROT 0897):
-- every call's inputs and the arm it took, so the engine kernel can be
-- replayed against them (docs/subsystems/motion-vm.md, "Bit 1 is not a
-- freeze").
--
--   * Exec BP 0x801D79E8 (entry): a0 = actor. Reads the inputs the routine
--     reads - _DAT_8007BAF4, actor +0x14 / +0x18 (X / Z), +0x58 (radius),
--     +0x16 (Y), +0x50 (id), +0x52, the focus pair _DAT_80089118 /
--     _DAT_80089120, the region box 0x1F800384..87 and the view window
--     0x1F8003E8..EB.
--   * Exec BPs on the three exits: 0x801D7A04 (forced visible), 0x801D7B08
--     (culled: bit 1 set), 0x801D7B1C (visible: bit 1 cleared).
--
-- Each distinct (inputs, arm) row is logged once, up to LEGAIA_CULL_ROWS;
-- per actor, a change of arm is always logged with the actor's +0x16 so a
-- held Y is visible. Optionally holds a d-pad direction (LEGAIA_CULL_WALK =
-- UP / DOWN / LEFT / RIGHT) for LEGAIA_CULL_WALK_FRAMES vsyncs so the
-- window sweeps across the placements.
--
-- Output: <LEGAIA_OUT_DIR>/cull.log, rows
--   `row id=.. x=.. z=.. r=.. f=(fx,fz) box=(..) win=(..) baf4=.. -> ARM`.
-- These guard only 0897's bytes: the BPs check the word at 0x801D79E8.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")
local bp = require("probe.bp")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/field_actor_cull")
local MAX_ROWS = probe.getenv_num("LEGAIA_CULL_ROWS", 600)
local WALK = probe.getenv("LEGAIA_CULL_WALK", "")
local WALK_FRAMES = probe.getenv_num("LEGAIA_CULL_WALK_FRAMES", 200)
os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/cull.log", "w")
local function log(s)
    PCSX.log("[cull] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local function u32(x) return (tonumber(x) or 0) % 0x100000000 end
local function s16(v) v = v or 0; if v >= 0x8000 then return v - 0x10000 end; return v end
local function s8(v) v = v or 0; if v >= 0x80 then return v - 0x100 end; return v end
local function s32(v) v = u32(v); if v >= 0x80000000 then return v - 0x100000000 end; return v end

local ENTRY_WORD = nil -- the first word of FUN_801D79E8 in 0897 (lui v0,0x8008)
local vs = 0
local cur = nil
local seen = {}
local rows = 0
local last_arm = {}

local function inputs(a)
    local sb = function(o) return probe.read_scratch_u8(0x1F800000 + o) end
    return {
        actor = a,
        id = probe.read_u16(a + 0x50) or 0,
        x = s16(probe.read_u16(a + 0x14)),
        y = s16(probe.read_u16(a + 0x16)),
        z = s16(probe.read_u16(a + 0x18)),
        r = s16(probe.read_u16(a + 0x58)),
        f52 = probe.read_u16(a + 0x52) or 0,
        fx = s32(probe.read_u32(0x80089118)),
        fz = s32(probe.read_u32(0x80089120)),
        box = { sb(0x384), sb(0x385), sb(0x386), sb(0x387) },
        win = { s8(sb(0x3E8)), s8(sb(0x3E9)), s8(sb(0x3EA)), s8(sb(0x3EB)) },
        baf4 = u32(probe.read_u32(0x8007BAF4)),
    }
end

local function finish(arm)
    if cur == nil then return end
    local c = cur
    cur = nil
    local key = string.format("x=%d z=%d r=%d f=(%d,%d) box=(%d,%d,%d,%d) win=(%d,%d,%d,%d) baf4=%d -> %s",
        c.x, c.z, c.r, c.fx, c.fz, c.box[1], c.box[2], c.box[3], c.box[4],
        c.win[1], c.win[2], c.win[3], c.win[4], c.baf4 ~= 0 and 1 or 0, arm)
    if not seen[key] and rows < MAX_ROWS then
        seen[key] = true
        rows = rows + 1
        log(string.format("row v=%d id=%X %s", vs, c.id, key))
    end
    if last_arm[c.actor] ~= arm then
        log(string.format("arm v=%d actor=%08X id=%X %s -> %s y=%d f52=%04X",
            vs, c.actor, c.id, tostring(last_arm[c.actor]), arm, c.y, c.f52))
        last_arm[c.actor] = arm
    end
end

local function guard()
    local w = probe.read_u32(0x801D79E8) or 0
    if ENTRY_WORD == nil then ENTRY_WORD = 0x3C028008 end
    return w == ENTRY_WORD
end

local held = {}
probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        bp.arm(0x801D79E8, "Exec", 4, "cull_entry", function()
            if not guard() then return end
            local r = PCSX.getRegisters()
            cur = inputs(u32(r.GPR.n.a0))
        end)
        bp.arm(0x801D7A04, "Exec", 4, "cull_forced", function()
            if guard() then finish("forced") end
        end)
        bp.arm(0x801D7B08, "Exec", 4, "cull_set", function()
            if guard() then finish("culled") end
        end)
        bp.arm(0x801D7B1C, "Exec", 4, "cull_clear", function()
            if guard() then finish("visible") end
        end)
        return {}
    end,

    on_capture = function(_ctx, tick)
        vs = tick
        -- Follow each culled actor's Y: a held Y does not change.
        if WALK ~= "" and pad.BTN[WALK] ~= nil then
            if tick == 30 then pad.force(pad.BTN[WALK]) end
            if tick == 30 + WALK_FRAMES then pad.release(pad.BTN[WALK]) end
        end
        if tick % 30 == 0 then
            for a, arm in pairs(last_arm) do
                local y = s16(probe.read_u16(a + 0x16))
                if held[a] ~= y then
                    log(string.format("y v=%d actor=%08X arm=%s y=%d", tick, a, arm, y))
                    held[a] = y
                end
            end
        end
    end,

    on_done = function()
        if WALK ~= "" and pad.BTN[WALK] ~= nil then pad.release(pad.BTN[WALK]) end
        log(string.format("=== field_actor_cull done: %d distinct rows ===", rows))
        if LOG then LOG:close() end
    end,
})
