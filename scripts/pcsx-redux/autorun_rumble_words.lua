-- autorun_rumble_words.lua
--
-- The DualShock rumble state, vsync by vsync, and which writer stored it.
--
-- Retail keeps the motor words at _DAT_8007B92C (pulse) / _DAT_8007B930
-- (level); the battle side stores a decay rate for each at gp+0xA30 /
-- gp+0xA34 (0x8007BD48 / 0x8007BD4C) and a hold timer at gp+0xA1C
-- (0x8007BD34), which the battle frame driver FUN_80046A20 runs down
-- (0x80047164..0x8004725C). The vsync callback FUN_80018DB0 turns the words
-- into port 0's actuator pair at 0x800915DA / 0x800915DB.
--
-- Logged every vsync: the game mode, the frame step DAT_1F800393, the five
-- words, the actuator pair, the three vibration option words (Battles
-- 0x800845C8, Events 0x800845A8, Encounters 0x800845CC; 0 = On) and the
-- battle context's command-flow / action-state bytes. Exec breakpoints on
-- the known writer sites log the vsync each one fired on.
--
-- Env vars:
--   LEGAIA_SSTATE      save state to load (else --scenario via run_probe.sh)
--   LEGAIA_FRAMES      vsyncs to capture after the load (default 900)
--   LEGAIA_HOLD_BTN    pad button name to hold (e.g. RIGHT); unset = no input
--   LEGAIA_HOLD_FRAMES vsyncs to hold it for (default 150)
--   LEGAIA_OUT_DIR     output directory (set by run_probe.sh)
--
-- Outputs (under LEGAIA_OUT_DIR):
--   rumble_words.csv   one row per vsync
--   rumble_writers.csv one row per writer-site hit
--
-- Run (an ambush that plays itself - monster hits on the party):
--   timeout 900 bash scripts/pcsx-redux/run_probe.sh \
--       --lua scripts/pcsx-redux/autorun_rumble_words.lua \
--       --scenario rim_elm_queen_bee_battle --frames 900 \
--       --isolate-config --out-dir <scratch>/rumble
--
-- Interpreter mode (the default): exec breakpoints do not fire under
-- --fast / --timing. Always wrap in `timeout` - probes do not exit.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local HOLD_BTN = probe.BTN[probe.getenv("LEGAIA_HOLD_BTN", "") or ""]
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD_FRAMES", 150)

local GAME_MODE_VA   = 0x8007B83C
local BATTLE_CTX_PTR = 0x8007BD24
local STEP_VA        = 0x1F800393

-- Writer sites: the first instruction of each store block.
local SITES = {
    { 0x801DE320, "finisher_party_struck" },
    { 0x801DE33C, "finisher_monster_struck" },
    { 0x801DEF24, "floor_part" },
    { 0x801EC8B0, "block_roll_party_defends" },
    { 0x801EC8EC, "block_roll_party_attacks" },
    { 0x801EC994, "block_window_party_defends" },
    { 0x801EC9D0, "block_window_party_attacks" },
    { 0x801ECAC4, "block_bd84_arm" },
    { 0x801EDCEC, "hit_party_struck" },
    { 0x801EDD28, "hit_party_attacks" },
    { 0x801EE538, "rot_party_struck" },
    { 0x8004A84C, "burning_body" },
    { 0x801DA708, "encounter_trigger" },
}

local words_csv, writers_csv
local elapsed_now = 0

local function u32(v) return (tonumber(v) or 0) % 0x100000000 end
local function s32(v)
    v = u32(v)
    if v >= 0x80000000 then return v - 0x100000000 end
    return v
end

local function ctx()
    local p = probe.read_u32(BATTLE_CTX_PTR)
    if p == nil then return nil end
    p = u32(p)
    if not probe.in_ram(p) then return nil end
    return p
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = FRAMES,
    hold_button = HOLD_BTN,
    hold_frames = HOLD_BTN and HOLD_FRAMES or 0,

    on_arm = function()
        words_csv = probe.csv_open(probe.out_path("rumble_words.csv"),
            "elapsed,mode,step,pulse,level,hold,rate_p,rate_l,act0,act1,"
            .. "opt_battles,opt_events,opt_encounters,flow,action")
        writers_csv = probe.csv_open(probe.out_path("rumble_writers.csv"),
            "elapsed,site,name,mode,ra")
        for _, s in ipairs(SITES) do
            local va, name = s[1], s[2]
            probe.arm_breakpoint(va, "Exec", 4, name, function()
                local r = PCSX.getRegisters()
                writers_csv:row("%d,0x%08X,%s,0x%02X,0x%08X", elapsed_now, va, name,
                    probe.read_u8(GAME_MODE_VA) or 0, u32(r.GPR.n.ra))
            end)
        end
        return {}
    end,

    on_capture = function(_, elapsed)
        elapsed_now = elapsed
        local c = ctx()
        words_csv:row("%d,0x%02X,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s,%s",
            elapsed,
            probe.read_u8(GAME_MODE_VA) or 0,
            probe.read_u8(STEP_VA) or 0,
            s32(probe.read_u32(0x8007B92C)),
            s32(probe.read_u32(0x8007B930)),
            s32(probe.read_u32(0x8007BD34)),
            s32(probe.read_u32(0x8007BD48)),
            s32(probe.read_u32(0x8007BD4C)),
            probe.read_u8(0x800915DA) or 0,
            probe.read_u8(0x800915DB) or 0,
            u32(probe.read_u32(0x800845C8)),
            u32(probe.read_u32(0x800845A8)),
            u32(probe.read_u32(0x800845CC)),
            c and string.format("0x%02X", probe.read_u8(c + 0x06) or 0) or "-",
            c and string.format("0x%02X", probe.read_u8(c + 0x07) or 0) or "-")
    end,

    on_summary = function()
        if words_csv then words_csv:close() end
        if writers_csv then writers_csv:close() end
        PCSX.log("[rumble] capture done")
    end,
})
