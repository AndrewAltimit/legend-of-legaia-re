-- autorun_boot_watch.lua
--
-- Minimal cold-boot watcher: no save state, no breakpoints, no pokes -
-- just the game-mode word sampled every 60 ticks. Used to bisect
-- boot-breaking disc patches (a healthy boot walks the mode through the
-- publisher logos into the title screen; a wedged one never leaves the
-- early modes).
--
-- Launch:
--   LEGAIA_NO_SSTATE=1 LEGAIA_FRAMES=6000 \
--   bash scripts/pcsx-redux/run_probe.sh --iso <disc.bin> \
--     --lua scripts/pcsx-redux/autorun_boot_watch.lua
--
-- Output: boot_watch.csv  tick,mode,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local GAME_MODE = 0x8007B83C
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)

local CSV = probe.csv_open(probe.out_path("boot_watch.csv"), "tick,mode,note")
local last_mode = -1

-- Cold boot: neuter the state loader (the trace-segment probe's idiom).
require("probe.sstate").load = function(_)
    PCSX.log("[boot-watch] cold boot; sstate load skipped")
    return true
end

probe.run({
    sstate = "unused",
    capture_frames = FRAMES,
    on_arm = function(ctx)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        if mode ~= last_mode then
            CSV:row("%d,0x%X,mode-change", elapsed, mode)
            last_mode = mode
        elseif elapsed % 600 == 0 then
            CSV:row("%d,0x%X,tick", elapsed, mode)
        end
    end,
    on_done = function()
        CSV:row("0,0x%X,done", last_mode)
        CSV:close()
    end,
})
