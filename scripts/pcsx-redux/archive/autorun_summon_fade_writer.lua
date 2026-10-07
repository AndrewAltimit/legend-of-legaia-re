-- autorun_summon_fade_writer.lua
--
-- The summon creature (slot 7) is drawn through the fade path FUN_8004A908,
-- but its fade byte `+0x226` reads 0 at draw time even after we force 0x40 the
-- previous vsync -- something RESETS it to 0 every frame (unlike the enemy's,
-- which persists). Find that writer: a Write breakpoint over the summon
-- actor's `+0x226` records every faulting PC + registers, so we know where the
-- per-frame reset lives and can hook just after it (set 0x40 after the reset,
-- before the draw) -- the real fix for Piece 1 (summon transparency).
--
-- Run (sstate7 = a mid Burning-Attack cast; summon = slot 7):
--   timeout 200 bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_summon_fade_writer.lua \
--     --sstate ~/Tools/pcsx-redux/SCUS94254.sstate7 --frames 20

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local ACTOR_TABLE = 0x801C9370
local SUMMON_SLOT = probe.getenv_num("LEGAIA_SUMMON_SLOT", 7)
local OUT = probe.out_path("summon_fade_writer.txt")

local f = assert(io.open(OUT, "w"))
local armed = false
local handle = nil
local g = 0

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 20),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g = elapsed
        if armed then
            -- keep forcing 0x40 so the resetter shows a 0x40->0 transition
            local ptr = probe.read_u32(ACTOR_TABLE + SUMMON_SLOT * 4) or 0
            if ptr ~= 0 then probe.write_u8(ptr + 0x226, 0x40) end
        end
        if not armed and elapsed >= 2 then
            local ptr = probe.read_u32(ACTOR_TABLE + SUMMON_SLOT * 4) or 0
            f:write(string.format("summon slot%d ptr=0x%08X  watching +0x226\n", SUMMON_SLOT, ptr))
            f:flush()
            probe.write_u8(ptr + 0x226, 0x40)
            local seen = {}
            -- watch a 4-byte window covering +0x224..+0x228 to catch wide stores
            handle = probe.step.find_writer(ptr + 0x224, 4, {
                read_len = 4,
                on_write = function(rg)
                    local key = string.format("%08X", rg.pc)
                    if seen[key] then return end
                    seen[key] = true
                    f:write(string.format(
                        "f=%-3d pc=0x%08X %s  s1=%08X s0=%08X a0=%08X v0=%08X v1=%08X\n",
                        g, rg.pc, rg.note or "", rg.s1 or 0, rg.s0 or 0, rg.a0 or 0, rg.v0 or 0, rg.v1 or 0))
                    f:flush()
                end,
            })
            armed = true
        end
        if elapsed >= (probe.getenv_num("LEGAIA_FRAMES", 20) - 2) then ctx.request_quit = true end
    end,
    on_done = function()
        if handle then handle:dump(OUT:gsub("%.txt$", ".records.txt")) end
        f:write("done\n"); f:close()
    end,
})
