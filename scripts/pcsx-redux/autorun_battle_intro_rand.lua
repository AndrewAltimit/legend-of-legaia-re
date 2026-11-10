-- autorun_battle_intro_rand.lua
--
-- Film one field-to-battle entry on retail: how many BIOS rand() draws the
-- transition init FUN_801CE8CC makes (and from which seeder), and the
-- battle camera's pose per vsync from the battle init to the end of the
-- frame driver's entry sweep.
--
--   1. load a retail field state (default kor_field_route_walk),
--   2. walk legs UP / RIGHT / DOWN / LEFT until an encounter rolls,
--   3. exec-BP the init entry 0x801CE8CC and its return 0x801CF1A8, the
--      three seeders it can call (FUN_801CFBB4 / FUN_801D0164 /
--      FUN_801D081C) and the rand thunk FUN_80056798, logging the BIOS seed
--      word 0x80009010 and the draws counted between them,
--   4. once the game mode reaches the battle, write one CSV row per vsync
--      of the entry counter gp+0x330 (0x8007B648), pitch / yaw / roll
--      (0x8007B790..94) and the TR trio (0x800840B8..C0), and quit 90
--      vsyncs after the counter parks at 0xFF.
--
--   bash scripts/pcsx-redux/run_probe.sh --headless \
--     --lua scripts/pcsx-redux/autorun_battle_intro_rand.lua \
--     --scenario kor_field_route_walk --frames 6000 --out-dir <dir>
--
-- Interpreter mode required (exec BPs).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)
local WALK_LEG = probe.getenv_num("LEGAIA_WALK_LEG", 50)

local GMODE     = 0x8007B83C
local SEED      = 0x80009010   -- BIOS rand() seed word (A(2Fh))
local RAND      = 0x80056798   -- the SCUS rand thunk
local INIT      = 0x801CE8CC   -- FUN_801CE8CC, transition init
local INIT_RET  = 0x801CF1A8   -- its jr ra
local TRANS     = 0x801CF5BC   -- FUN_801CF5BC, the transition tick
local STYLE_SEL = 0x801D2460
local SUB_STYLE = 0x801D2464
local SEEDERS = {
    [0x801CFBB4] = "seed_cfbb4",
    [0x801D0164] = "seed_d0164",
    [0x801D081C] = "seed_d081c",
}
local COUNTER = 0x8007B648
local PITCH   = 0x8007B790
local YAW     = 0x8007B792
local ROLL    = 0x8007B794
local TR      = 0x800840B8

local function toi16(v) v = v or 0; if v >= 0x8000 then return v - 0x10000 end return v end
local function toi32(v) v = v or 0; if v >= 0x80000000 then return v - 0x100000000 end return v end

local log = probe.csv_open(probe.out_path("intro_rand.csv"),
    "vsync,event,seed,draws_total,draws_since_init,style,sub,ra")
local cam = probe.csv_open(probe.out_path("entry_camera.csv"),
    "vsync,gmode,counter,pitch,yaw,roll,tr_x,tr_y,tr_z,seed,draws_total")

local vsync_now = 0
local draws_total = 0
local in_init = false
local init_draws = 0
local trans_ticks = 0
local parked_at = nil
local held = nil
local last_mode = nil

local function event(name, ra)
    log:row("%d,%s,0x%08X,%d,%d,%d,%d,0x%08X", vsync_now, name,
        probe.read_u32(SEED) or 0, draws_total, init_draws,
        toi32(probe.read_u32(STYLE_SEL)), toi32(probe.read_u32(SUB_STYLE)), ra or 0)
end

-- The overlay window is shared: a BP address only names the intro's code
-- while PROT 0979 is resident there, so each hit checks the first word.
local SIG = {
    [INIT] = 0x27BDFFB8, [INIT_RET] = 0x03E00008, [TRANS] = 0x27BDFFD8,
    [0x801CFBB4] = 0x27BDFFC0, [0x801D0164] = 0x27BDFFC0, [0x801D081C] = 0x27BDFFB0,
}
local function resident(addr) return probe.read_u32(addr) == SIG[addr] and probe.read_u32(INIT) == SIG[INIT] end

local function arm()
    probe.arm_breakpoint(RAND, "Exec", 4, "rand", function()
        draws_total = draws_total + 1
        if in_init then init_draws = init_draws + 1 end
    end)
    probe.arm_breakpoint(INIT, "Exec", 4, "init", function()
        if not resident(INIT) then return end
        in_init = true
        init_draws = 0
        local r = PCSX.getRegisters()
        event("init_entry", r.GPR.n.ra)
    end)
    probe.arm_breakpoint(INIT_RET, "Exec", 4, "init_ret", function()
        if not in_init then return end
        event("init_return", 0)
        in_init = false
        -- The tile shatter's record block (FUN_801D081C stores its pointer
        -- at 0x801D2468): 256 records of 0x5C bytes, the seeded delays at
        -- +0x0A. Dumped raw beside the CSV for an offline comparison.
        local blk = probe.read_u32(0x801D2468) or 0
        if blk >= 0x80000000 and blk < 0x80200000 then
            local bytes = probe.read_bytes(blk, 0x5C00)
            local fh = bytes and io.open(probe.out_path("tile_records.bin"), "wb")
            if fh then fh:write(tostring(bytes)); fh:close() end
        end
    end)
    for addr, name in pairs(SEEDERS) do
        probe.arm_breakpoint(addr, "Exec", 4, name, function()
            if in_init then event(name, 0) end
        end)
    end
    probe.arm_breakpoint(TRANS, "Exec", 4, "trans", function()
        if not resident(TRANS) then return end
        trans_ticks = trans_ticks + 1
        if trans_ticks <= 3 then event("trans_tick", 0) end
    end)
end

local BTN = probe.BTN
local DIRS = {
    { BTN.UP }, { BTN.RIGHT }, { BTN.DOWN }, { BTN.LEFT },
    { BTN.UP, BTN.RIGHT }, { BTN.RIGHT, BTN.DOWN }, { BTN.DOWN, BTN.LEFT }, { BTN.LEFT, BTN.UP },
}
local lcg = probe.getenv_num("LEGAIA_WALK_SEED", 7)
local function rnd(n)
    lcg = (lcg * 1103515245 + 12345) % 2147483648
    return math.floor(lcg / 65536) % n
end
local walk_next = 0
local function release_all()
    if held ~= nil then for _, b in ipairs(held) do probe.pad_release(b) end end
    held = nil
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function(c)
        probe.write_manifest("autorun_battle_intro_rand.lua", {
            sstate = SSTATE_PATH, frames = FRAMES,
        })
        arm()
        return {}
    end,
    on_capture = function(c, vsync)
        vsync_now = vsync
        local mode = probe.read_u8(GMODE) or 0
        if mode ~= last_mode then
            event(string.format("mode_%02X", mode), 0)
            last_mode = mode
        end
        if mode == 0x03 and trans_ticks == 0 then
            if vsync >= walk_next then
                release_all()
                held = DIRS[rnd(#DIRS) + 1]
                for _, b in ipairs(held) do probe.pad_force(b) end
                walk_next = vsync + 30 + rnd(121)
            end
        elseif held ~= nil then
            release_all()
        end
        if trans_ticks > 0 then
            local counter = probe.read_u8(COUNTER) or 0
            cam:row("%d,0x%02X,0x%02X,%d,%d,%d,%d,%d,%d,0x%08X,%d", vsync, mode, counter,
                toi16(probe.read_u16(PITCH)), toi16(probe.read_u16(YAW)),
                toi16(probe.read_u16(ROLL)), toi32(probe.read_u32(TR)),
                toi32(probe.read_u32(TR + 4)), toi32(probe.read_u32(TR + 8)),
                probe.read_u32(SEED) or 0, draws_total)
            if (mode == 0x14 or mode == 0x15) and counter == 0xFF and parked_at == nil then
                parked_at = vsync
            end
            if parked_at ~= nil and vsync >= parked_at + 90 then
                c.request_quit = true
            end
        end
    end,
    on_done = function(c)
        PCSX.log(string.format("[intro_rand] done: trans_ticks=%d draws_total=%d",
            trans_ticks, draws_total))
    end,
})
