-- autorun_delilas_battle_load.lua
--
-- Can retail's battle loader stage a formation of N DISTINCT monster ids?
-- Retail authoring never exceeds 2 distinct ids per formation, and the
-- Delilas Challenge patch stages 3 distinct bosses (162/163/164, ~345 KB of
-- decoded blocks) - a freeze was reported at the battle-load transition.
-- This probe isolates the loader question completely: from a plain field
-- save it replays what FUN_801DA51C states 1/2 do on a rolled encounter
-- (install the formation cell 0x8007BD0C..0F, store game_mode = 8) with an
-- arbitrary id list, then watches the mode chain (8 -> 9 -> 0x14 -> 0x15)
-- and the actor table. Verdict per run: BATTLE_MAIN reached and stable, or
-- stuck (with the mode it stalled in).
--
-- Launch (ids comma-separated, up to 4):
--   LEGAIA_IDS=162,163,164 LEGAIA_FRAMES=1800 \
--   timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh \
--     --scenario field_walled_collision_pin \
--     --lua scripts/pcsx-redux/autorun_delilas_battle_load.lua
--
-- Output: delilas_battle_load.csv  tick,mode,actors,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local GAME_MODE      = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local ACTOR_TABLE    = 0x801C9370 -- 8 x u32 actor pointers
local BATTLE_MAIN    = 0x15

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1800)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local IDS_RAW  = probe.getenv("LEGAIA_IDS", "162,163,164")

local ids = {}
for tok in string.gmatch(IDS_RAW, "[^,%s]+") do
    ids[#ids + 1] = tonumber(tok)
end

local CSV = probe.csv_open(probe.out_path("delilas_battle_load.csv"),
    "tick,mode,actors,note")

local function actors_seated()
    local n = 0
    for i = 0, 7 do
        local p = probe.read_u32(ACTOR_TABLE + i * 4)
        if p ~= nil and p ~= 0 then n = n + 1 end
    end
    return n
end

local installed = false
local reached_main = false
local verdict_written = false
local last_mode = -1
local settle = 0

local function verdict(note, mode)
    if verdict_written then return end
    verdict_written = true
    CSV:row("0,0x%X,%d,%s", mode, actors_seated(), note)
    CSV:close()
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        return {}
    end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        if elapsed == FORCE_AT then
            for i = 0, 3 do
                probe.write_u8(FORMATION_CELL + i, ids[i + 1] or 0)
            end
            probe.write_u16(GAME_MODE, 8)
            installed = true
            CSV:row("%d,0x%X,%d,installed ids=%s", elapsed, mode,
                actors_seated(), IDS_RAW)
            return
        end
        if mode ~= last_mode then
            CSV:row("%d,0x%X,%d,mode-change", elapsed, mode, actors_seated())
            last_mode = mode
        end
        if installed and mode == BATTLE_MAIN then
            reached_main = true
            settle = settle + 1
            if settle > 240 then
                verdict("VERDICT: battle loads and runs", mode)
                ctx.request_quit = true
            end
        end
    end,
    on_done = function()
        if reached_main then
            verdict("VERDICT: battle loads and runs", last_mode)
        else
            verdict("VERDICT: never reached battle main - stuck", last_mode)
        end
    end,
})
