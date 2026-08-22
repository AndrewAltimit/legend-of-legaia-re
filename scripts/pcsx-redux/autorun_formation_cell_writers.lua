-- autorun_formation_cell_writers.lua
--
-- Who rewrites the enemy formation cells (DAT_8007BD0C..0F) between the
-- encounter install and battle main? Forces a formation + game_mode 8 like
-- autorun_delilas_battle_load.lua, then arms WRITE watchpoints on the four
-- cell bytes and logs every writer PC with the byte it wrote. Interpreter
-- mode required (Lua breakpoints never fire under the dynarec).
--
-- Launch:
--   LEGAIA_IDS=133,151,94 LEGAIA_FRAMES=3000 \
--   timeout --kill-after=30s 1500s bash scripts/pcsx-redux/run_probe.sh \
--     --scenario karisto_sol_pre_encounter \
--     --lua scripts/pcsx-redux/autorun_formation_cell_writers.lua
--
-- Output: formation_cell_writers.csv  tick,mode,cell,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local GAME_MODE      = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local BATTLE_MAIN    = 0x15

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 3000)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local IDS_RAW  = probe.getenv("LEGAIA_IDS", "133,151,94")

local ids = {}
for tok in string.gmatch(IDS_RAW, "[^,%s]+") do
    ids[#ids + 1] = tonumber(tok)
end

local CSV = probe.csv_open(probe.out_path("formation_cell_writers.csv"),
    "tick,mode,cell,note")

local installed = false
local last_mode = -1
local settled = 0

local function reg_pc()
    local ok, v = pcall(function()
        return tonumber(PCSX.getRegisters().pc) % 0x100000000
    end)
    if ok then return v end
    return 0
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function(ctx)
        for i = 0, 3 do
            probe.arm_breakpoint(FORMATION_CELL + i, "Write", 1,
                "cell" .. i, function()
                    if not installed then return end
                    local v = probe.read_u8(FORMATION_CELL + i) or 255
                    CSV:row("0,0,%d,WRITE pc=0x%08X now=%d", i, reg_pc(), v)
                end)
        end
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
            CSV:row("%d,0x%X,0,installed ids=%s", elapsed, mode, IDS_RAW)
            return
        end
        if mode ~= last_mode then
            CSV:row("%d,0x%X,0,mode-change", elapsed, mode)
            last_mode = mode
        end
        if installed and mode == BATTLE_MAIN then
            settled = settled + 1
            if settled == 60 then
                local c = {}
                for i = 0, 3 do c[i + 1] = probe.read_u8(FORMATION_CELL + i) or 255 end
                CSV:row("%d,0x%X,0,cells at main = %d %d %d %d",
                    elapsed, mode, c[1], c[2], c[3], c[4])
                ctx.request_quit = true
            end
        end
    end,
    on_done = function()
        CSV:row("0,0,0,done")
        CSV:close()
    end,
})
