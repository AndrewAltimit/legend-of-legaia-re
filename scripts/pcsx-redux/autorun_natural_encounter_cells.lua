-- autorun_natural_encounter_cells.lua
--
-- End-to-end natural-encounter observation: from a field/world-map save
-- state parked near an encounter roll, hold a direction on the pad until
-- the game itself rolls a battle (no formation poke, no mode poke), then
-- log the formation cells at every mode change and at battle main, plus
-- the per-enemy record pointers. This is the ground-truth companion to
-- autorun_delilas_battle_load.lua's forced-install path: it shows what a
-- REAL rolled encounter installs and what the loader does with it.
--
-- LEGAIA_DIR: pad direction to hold (default "right").
--
-- Output: natural_encounter_cells.csv  tick,mode,actors,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE      = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local ACTOR_TABLE    = 0x801C9370
local RECORD_TABLE   = 0x801C9348
local BATTLE_MAIN    = 0x15

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3600)
local DIR    = probe.getenv("LEGAIA_DIR", "right")

local DIRBTN = ({ up = pad.BTN.UP, down = pad.BTN.DOWN,
                  left = pad.BTN.LEFT, right = pad.BTN.RIGHT })[DIR]

local CSV = probe.csv_open(probe.out_path("natural_encounter_cells.csv"),
    "tick,mode,actors,note")

local function cells()
    local c = {}
    for i = 0, 3 do c[i + 1] = probe.read_u8(FORMATION_CELL + i) or 255 end
    return string.format("%d %d %d %d", c[1], c[2], c[3], c[4])
end

local function actors_seated()
    local n = 0
    for i = 0, 7 do
        local p = probe.read_u32(ACTOR_TABLE + i * 4)
        if p ~= nil and p ~= 0 then n = n + 1 end
    end
    return n
end

local last_mode = -1
local settled = 0
local done = false

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        -- Walk until the game leaves field mode on its own.
        if mode == 0x03 and not done then
            pad.force(DIRBTN)
        else
            pad.release(DIRBTN)
        end
        if mode ~= last_mode then
            CSV:row("%d,0x%X,%d,mode-change cells=%s", elapsed, mode,
                actors_seated(), cells())
            last_mode = mode
        end
        if mode == BATTLE_MAIN and not done then
            settled = settled + 1
            if settled == 60 then
                CSV:row("%d,0x%X,%d,cells at main = %s", elapsed, mode,
                    actors_seated(), cells())
                for i = 0, 4 do
                    local r = probe.read_u32(RECORD_TABLE + i * 4) or 0
                    if r ~= 0 then
                        CSV:row("0,0,0,record[%d]=0x%X", i, r)
                    end
                end
                done = true
                ctx.request_quit = true
            end
        end
    end,
    on_done = function()
        if not done then
            CSV:row("0,0,0,no battle rolled within frame budget")
        end
        CSV:close()
    end,
})
