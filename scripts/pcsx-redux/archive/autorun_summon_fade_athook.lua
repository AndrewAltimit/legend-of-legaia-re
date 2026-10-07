-- autorun_summon_fade_athook.lua
--
-- The summon actor's +0x226 reads 0 at draw time no matter what we pre-write
-- (a per-frame struct rebuild we can't trap clears it). So the only viable fix
-- is to inject the fade AT THE READ. This probe simulates exactly what a detour
-- at 0x8004AD0C (`lbu v0,0x226(s1)` in FUN_8004A908) would do: an Exec BP fires
-- *before* the lbu; if s1 is the summon actor, write 0x40 into s1+0x226 so the
-- lbu reads 0x40. If the creature then renders translucent, the at-read detour
-- is the correct design for Piece 1.
--
-- Run: LEGAIA_SUMMON_SLOT=7 timeout 220 bash scripts/pcsx-redux/run_probe.sh \
--   --lua scripts/pcsx-redux/autorun_summon_fade_athook.lua \
--   --sstate ~/Tools/pcsx-redux/SCUS94254.sstate7 --frames 30

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local ACTOR_TABLE = 0x801C9370
local SUMMON_SLOT = probe.getenv_num("LEGAIA_SUMMON_SLOT", 7)
local FADE_READER = 0x8004AD0C
local FORCE_FADE  = probe.getenv_num("LEGAIA_FORCE_FADE", 0x40)

local summon_ptr = 0

local function take_fb(stem, label)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then return end
    local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh then fh:write(tostring(ss.data)); fh:close() end
    local mh = io.open(probe.out_path(stem .. ".meta"), "w")
    if mh then mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
        tonumber(ss.width), tonumber(ss.height), bpp)); mh:close() end
    PCSX.log(string.format("[athook] wrote %s", stem))
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 30),
    on_arm = function()
        probe.arm_breakpoint(FADE_READER, "Exec", 4, "fade_read", function()
            if summon_ptr == 0 then return end
            local r = PCSX.getRegisters()
            local s1 = (tonumber(r.GPR.n.s1) or 0) % 0x100000000
            if s1 == summon_ptr then
                probe.write_u8(s1 + 0x226, FORCE_FADE)  -- lbu will read this
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        if elapsed == 2 then
            summon_ptr = probe.read_u32(ACTOR_TABLE + SUMMON_SLOT * 4) or 0
            PCSX.log(string.format("[athook] summon slot%d ptr=0x%08X force=0x%02X",
                SUMMON_SLOT, summon_ptr, FORCE_FADE))
        end
        if elapsed == 8  then take_fb("athook_f8",  "F8")  end
        if elapsed == 16 then take_fb("athook_f16", "F16") end
        if elapsed == 24 then take_fb("athook_f24", "F24"); ctx.request_quit = true end
    end,
    on_summary = function() PCSX.log("[athook] done") end,
})
