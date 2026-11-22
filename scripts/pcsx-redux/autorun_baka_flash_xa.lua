-- autorun_baka_flash_xa.lua
--
-- Baka Fighter: the pending-critical flash and the CD-XA in-flight word,
-- vsync by vsync, from a duel state on the retail disc.
--
-- Pad only: Square / Circle / Cross in turn every LEGAIA_BAKA_PERIOD vsyncs
-- (default 24) so the exchanges run, with Cross also clearing the banners.
-- No pokes unless LEGAIA_BAKA_HP is set: then at every vsync both fighters'
-- HP above that value are written down to it (0x801DBFC4 / +0xA8), so the
-- comeback critical (own HP below 0x280) can arm; say so wherever the
-- capture is cited.
--
-- Writes flash.csv, one row per vsync: the cabinet state DAT_801DBF44, the
-- cabinet frame counter DAT_801DC130, the scene clock DAT_801DC128, each
-- fighter's HP, critical-pending flag (+0xA0) and actor colour word +0x74 /
-- depth-cue level +0x78, the CD-XA word _DAT_8007BC20, and the all-clear
-- announcer state DAT_801DC134, its timer DAT_801DC138, the all-clear fades
-- DAT_801DBEB0 / DAT_801DBEB4, the state timer DAT_801DBF88 and the frame
-- step 0x1F800393. Breakpoints log every clip start
-- FUN_8003D53C (a0..a2) into xa.log. Run without --fast.
--
-- Env: LEGAIA_SSTATE (via --scenario), LEGAIA_FRAMES, LEGAIA_BAKA_PERIOD,
-- LEGAIA_BAKA_HP; LEGAIA_BAKA_OPP_HP caps the opponent's HP alone each vsync,
-- and LEGAIA_BAKA_STAGE writes the stage counter DAT_801DC10C at the first
-- vsync (0xD: the next rung won raises the all-clear). Both are pokes.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)
local PERIOD = probe.getenv_num("LEGAIA_BAKA_PERIOD", 24)
local HP_CAP = probe.getenv_num("LEGAIA_BAKA_HP", 0)
local OPP_HP = probe.getenv_num("LEGAIA_BAKA_OPP_HP", 0)
local STAGE = probe.getenv_num("LEGAIA_BAKA_STAGE", -1)
local BTN = probe.BTN
local ARMS = { BTN.SQUARE, BTN.CIRCLE, BTN.CROSS }

local csv, xa
local vs = 0
local held, arm_i = nil, 0

local function actor(p)
    local a = probe.read_u32(p) or 0
    if a >= 0x80000000 and a < 0x80200000 then return a end
    return nil
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function(ctx)
        csv = probe.csv_open(probe.out_path("flash.csv"),
            "v,cab,ctr,clock,hp0,crit0,col0,lvl0,hp1,crit1,col1,lvl1,xa,clear,ctimer,fade_in,fade_out,stimer,step")
        xa = io.open(probe.out_path("xa.log"), "w")
        local bp = require("probe.bp")
        bp.arm(0x8003D53C, "Exec", 4, "xa_start", function()
            local g = PCSX.getRegisters().GPR.n
            xa:write(string.format("v=%d xa_start a0=%d a1=%d a2=%d ra=0x%08x word=%d\n", vs,
                tonumber(g.a0) or 0, tonumber(g.a1) or 0, tonumber(g.a2) or 0,
                tonumber(g.ra) or 0, probe.read_u32(0x8007BC20) or -1))
            xa:flush()
        end)
        return {}
    end,
    on_capture = function(ctx, vsync)
        vs = vsync
        if vsync == 1 and STAGE >= 0 then probe.write_u32(0x801DC10C, STAGE) end
        if OPP_HP > 0 then
            local hp = probe.read_u32(0x801DC06C) or 0
            if hp > OPP_HP and hp < 0x10000 then probe.write_u32(0x801DC06C, OPP_HP) end
        end
        if HP_CAP > 0 then
            for s = 0, 1 do
                local a = 0x801DBFC4 + s * 0xA8
                local hp = probe.read_u32(a) or 0
                if hp > HP_CAP and hp < 0x10000 then probe.write_u32(a, HP_CAP) end
            end
        end
        if vsync % PERIOD == 0 then
            if held ~= nil then probe.pad.release(held) end
            arm_i = arm_i % #ARMS + 1
            held = ARMS[arm_i]
            probe.pad.force(held)
        elseif held ~= nil and vsync % PERIOD == 4 then
            probe.pad.release(held)
            held = nil
        end
        local a0, a1 = actor(0x801DBFA0), actor(0x801DBFA4)
        csv:row("%d,%d,%d,%d,%d,%d,0x%08x,%d,%d,%d,0x%08x,%d,%d,%d,%d,%d,%d,%d,%d", vsync,
            probe.read_u32(0x801DBF44) or -1, probe.read_u32(0x801DC130) or -1,
            probe.read_u32(0x801DC128) or -1,
            probe.read_u32(0x801DBFC4) or -1, probe.read_u32(0x801DC05C) or -1,
            a0 and probe.read_u32(a0 + 0x74) or 0, a0 and probe.read_u32(a0 + 0x78) or -1,
            probe.read_u32(0x801DBFC4 + 0xA8) or -1, probe.read_u32(0x801DC05C + 0xA8) or -1,
            a1 and probe.read_u32(a1 + 0x74) or 0, a1 and probe.read_u32(a1 + 0x78) or -1,
            probe.read_u32(0x8007BC20) or -1, probe.read_u32(0x801DC134) or -1,
            probe.read_u32(0x801DC138) or -1, probe.read_u32(0x801DBEB0) or -1,
            probe.read_u32(0x801DBEB4) or -1, probe.read_u32(0x801DBF88) or -1,
            probe.read_scratch_u8(0x1F800393) or -1)
    end,
    on_done = function()
        if xa then xa:close() end
    end,
})
