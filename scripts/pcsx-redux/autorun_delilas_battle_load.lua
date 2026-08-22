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
local pad = require("probe.pad")

local GAME_MODE      = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local ACTOR_TABLE    = 0x801C9370 -- 8 x u32 actor pointers
local BATTLE_MAIN    = 0x15

-- Custom 2-pool heap (FUN_8002b3d4 init / FUN_8002b468 alloc); gp=0x8007B318.
local HEAP_DESC_PTR  = 0x8007BB58 -- gp+0x840: -> descriptor {TOP, pool_count}
local MALLOC_ERR     = 0x8007B828 -- gp+0x510: += 0x20000 per failed malloc
local LOADER_SUBSTATE = 0x8007BD71 -- gp+0xa59: battle scene-loader SM byte
local RECORD_TABLE   = 0x801C9348 -- per-enemy decoded-record pointers

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1800)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local IDS_RAW  = probe.getenv("LEGAIA_IDS", "162,163,164")
-- Optional party-roster override (comma list of character ids, e.g. "1" =
-- Vahn only). Written to DAT_8007BD10[0..2] at install time; the battle
-- setup FUN_80055B6C only re-seeds that table when byte 0 is zero, and the
-- party-pack loader FUN_80052FA0 skips zero roster bytes - so this sheds
-- the benched members' ~62 KB battle packs from the heap, the same net
-- effect as the field-VM 0x3D PARTY_REMOVE idiom the ravine duels use.
local PARTY_RAW = probe.getenv("LEGAIA_PARTY", "")
local PARTY_TABLE = 0x8007BD10
-- Optional RAM pokes applied at install time, "addr:word,addr:word,..."
-- (hex or decimal). Used to dress-rehearse the shipped Delilas stream-map
-- hook: the save state carries vanilla SCUS code in RAM, so the cave routine
-- + the `j` hook word (read out of the patched disc's SCUS) are poked in
-- here while the CLONE SLOTS stream from the patched --iso image.
-- Mash mode: once battle main is reached, press through the command flow so
-- moves execute (see the battle-main block). 0 = idle (load-only verdict).
local MASH = probe.getenv_num("LEGAIA_MASH", 0)
local POKES_RAW = probe.getenv("LEGAIA_POKES", "")
local pokes = {}
for pair in string.gmatch(POKES_RAW, "[^,%s]+") do
    local a, v = string.match(pair, "([^:]+):(.+)")
    if a and v then
        pokes[#pokes + 1] = { addr = tonumber(a), val = tonumber(v) }
    end
end

local ids = {}
for tok in string.gmatch(IDS_RAW, "[^,%s]+") do
    ids[#ids + 1] = tonumber(tok)
end
local party = {}
for tok in string.gmatch(PARTY_RAW, "[^,%s]+") do
    party[#party + 1] = tonumber(tok)
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

-- Walk the heap free ring + report the malloc-err accumulator and the
-- per-enemy record table, so a stuck verdict names WHERE the load died.
local function heap_report(tag)
    local desc = probe.read_u32(HEAP_DESC_PTR)
    if desc == nil or desc == 0 then
        CSV:row("0,0,0,%s heap: no descriptor", tag)
        return
    end
    local top = probe.read_u32(desc) or 0
    local err = probe.read_u32(MALLOC_ERR) or 0
    local sub = probe.read_u8(LOADER_SUBSTATE) or 0
    CSV:row("0,0,0,%s heap desc=0x%X top=0x%X malloc_err=0x%X loader_sub=0x%X",
        tag, desc, top, err, sub)
    local sent = top + 0xC
    local node = probe.read_u32(top + 0x10)
    local total, largest, n = 0, 0, 0
    while node ~= nil and node ~= sent and n < 64 do
        local size = probe.read_u32(node + 8) or 0
        total = total + size
        if size > largest then largest = size end
        n = n + 1
        CSV:row("0,0,0,%s free node @0x%X size=0x%X", tag, node, size)
        node = probe.read_u32(node + 4)
    end
    CSV:row("0,0,0,%s free total=0x%X (%d KB) largest=0x%X nodes=%d",
        tag, total, math.floor(total / 1024), largest, n)
    for i = 0, 4 do
        local r = probe.read_u32(RECORD_TABLE + i * 4) or 0
        if r ~= 0 then
            CSV:row("0,0,0,%s record[%d]=0x%X", tag, i, r)
        end
    end
end

local function verdict(note, mode)
    if verdict_written then return end
    verdict_written = true
    CSV:row("0,0x%X,%d,%s", mode, actors_seated(), note)
    -- Formation-cell readback: shows whether anything rewrote the installed
    -- ids between install time and the verdict (dedupe/aliasing forensics).
    local c0 = probe.read_u8(FORMATION_CELL) or 0
    local c1 = probe.read_u8(FORMATION_CELL + 1) or 0
    local c2 = probe.read_u8(FORMATION_CELL + 2) or 0
    local c3 = probe.read_u8(FORMATION_CELL + 3) or 0
    CSV:row("0,0,0,cells at verdict = %d %d %d %d", c0, c1, c2, c3)
    heap_report("at-verdict")
    CSV:close()
end

-- Allocator instrumentation. FUN_80017888 is the malloc wrapper
-- (a0 = pool, a1 = size); its failure branch falls through to 0x800178B8
-- only when the best-fit alloc FUN_8002B468 returned NULL. A hang at
-- battle load stops vsync callbacks entirely, so these breakpoints are
-- the only way to see the failing allocation and the heap at that moment.
local MALLOC_ENTRY = 0x80017888
local MALLOC_FAIL  = 0x800178B8

local function reg(r, name)
    local ok, v = pcall(function() return tonumber(r.GPR.n[name]) % 0x100000000 end)
    if ok then return v end
    return 0
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function(ctx)
        probe.arm_breakpoint(MALLOC_ENTRY, "Exec", 4, "malloc", function()
            if not installed then return end
            local r = PCSX.getRegisters()
            CSV:row("0,0,0,malloc pool=%d size=0x%X ra=0x%08X",
                reg(r, "a0"), reg(r, "a1"), reg(r, "ra"))
        end)
        probe.arm_breakpoint(MALLOC_FAIL, "Exec", 4, "malloc_fail", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,0,MALLOC FAILED size=0x%X", reg(r, "s1"))
            heap_report("at-malloc-fail")
            verdict("VERDICT: malloc failed during battle load", last_mode)
            ctx.request_quit = true
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        if elapsed == FORCE_AT then
            heap_report("pre-install")
            for _, p in ipairs(pokes) do
                probe.write_u32(p.addr, p.val)
                CSV:row("%d,0x%X,0,poke 0x%08X=0x%08X", elapsed, mode, p.addr, p.val)
            end
            if #party > 0 then
                for i = 0, 2 do
                    probe.write_u8(PARTY_TABLE + i, party[i + 1] or 0)
                end
                CSV:row("%d,0x%X,0,party override=%s", elapsed, mode, PARTY_RAW)
            end
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
            if MASH == 0 and settle > 240 then
                verdict("VERDICT: battle loads and runs", mode)
                ctx.request_quit = true
            end
            -- Mash mode: drive the command flow so moves actually execute
            -- (the idle probe never leaves the command menu, which is how a
            -- first-action defect can hide behind a "loads and runs"
            -- verdict). Alternate short presses; report the heap
            -- periodically so an in-battle OOM shows its slope.
            if MASH ~= 0 then
                local phase = settle % 40
                if phase == 0 then
                    pad.force(pad.BTN.CROSS)
                elseif phase == 6 then
                    pad.release(pad.BTN.CROSS)
                elseif phase == 20 then
                    pad.force(pad.BTN.UP)
                elseif phase == 26 then
                    pad.release(pad.BTN.UP)
                end
                if settle % 300 == 1 then
                    CSV:row("%d,0x%X,%d,mash-tick", elapsed, mode, actors_seated())
                    heap_report("mash")
                end
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
