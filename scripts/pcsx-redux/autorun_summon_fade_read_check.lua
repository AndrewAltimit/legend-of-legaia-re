-- autorun_summon_fade_read_check.lua
--
-- Forcing the summon actor's (slot 7) +0x226 = 0x40 produced ZERO change in
-- the rendered frame, despite the actor being active and drawn through the
-- fade path. Settle WHY: breakpoint the actual fade read (0x8004AD0C,
-- `lbu v0,0x226(s1)` in FUN_8004A908) and, for every hit, log the actor s1,
-- the fade byte just read (v0), the draw-object s3 (=a0 at entry; here in s3),
-- and whether s1 == the summon. If the read returns 0x40 but pixels don't
-- change -> the modulated colour at s3+0x74 isn't consumed; if s1 never equals
-- slot 7 -> the creature mesh is drawn by a different actor/path.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local ACTOR_TABLE = 0x801C9370
local SUMMON_SLOT = probe.getenv_num("LEGAIA_SUMMON_SLOT", 7)
local FADE_READER = 0x8004AD0C
local OUT = probe.out_path("summon_fade_read_check.txt")

local f = assert(io.open(OUT, "w"))
local summon_ptr = nil
local rows = {}

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 30),
    on_arm = function()
        probe.arm_breakpoint(FADE_READER, "Exec", 4, "fade_read", function()
            local r = PCSX.getRegisters()
            local s1 = (tonumber(r.GPR.n.s1) or 0) % 0x100000000
            local s3 = (tonumber(r.GPR.n.s3) or 0) % 0x100000000
            local fade = (s1 >= 0x80000000 and s1 < 0x80200000) and (probe.read_u8(s1 + 0x226) or 0) or -1
            local key = string.format("%08X", s1)
            local e = rows[key]
            if e then e.hits = e.hits + 1 else
                rows[key] = { s1 = s1, s3 = s3, fade = fade, hits = 1 }
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        if elapsed == 2 then
            summon_ptr = probe.read_u32(ACTOR_TABLE + SUMMON_SLOT * 4) or 0
            f:write(string.format("summon slot%d ptr=0x%08X\n", SUMMON_SLOT, summon_ptr))
        end
        if summon_ptr and summon_ptr ~= 0 then probe.write_u8(summon_ptr + 0x226, 0x40) end
        if elapsed >= probe.getenv_num("LEGAIA_FRAMES", 30) - 2 then ctx.request_quit = true end
    end,
    on_summary = function()
        f:write("s1(actor)   s3(drawobj)  fade_read  hits  is_summon\n")
        for _, e in pairs(rows) do
            f:write(string.format("0x%08X  0x%08X   0x%02X      %5d  %s\n",
                e.s1, e.s3, bit.band(e.fade, 0xff), e.hits,
                (summon_ptr and e.s1 == summon_ptr) and "YES" or ""))
        end
        f:close()
    end,
})
