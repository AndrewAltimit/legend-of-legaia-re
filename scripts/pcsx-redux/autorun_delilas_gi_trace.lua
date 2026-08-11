-- autorun_delilas_gi_trace.lua
--
-- Drive the Delilas course through round 0 (Che & Lu weakened to 1 HP) into
-- the Gi round and observe the HP-gated Divide / Spore Gas / Blazing Slash
-- lockout arm:
--   - round 0 (word 0x131): per-slot one-shot weaken so the mash clears it,
--   - round 1 (word 0x132): once the shared turn counter reaches
--     LEGAIA_GI_WEAKEN_TURN (default 6; 0 disables), Gi (slot 3) is
--     one-shot weakened to max/4 - strictly below the half-HP Divide gate -
--     so the trace shows the pre-weaken stock cadence (Spore Gas at counter
--     3, Blazing Slash 0x79 at counter 5) and then the Divide + lockout.
--     Enemy raw +0x14C writes are safe (round-0 precedent); do NOT boost
--     the player's HP: the party displayed-HP mirror (+0x172) is
--     DELTA-driven, a raw write desyncs it forever and the 0x51 exit gate
--     parks the battle (LEGAIA_PLAYER_HP=n re-enables for wedge-shape
--     experiments only),
--   - exec BP on the Gi cave's queue (GI_ARM_VA + 39*4 = 0x80050DDC) logs
--     the queued spell id + turn counter - the cast receipt,
--   - per-tick actor rows (anim pair / action id / HP) show the cast playing
--     out, a Divide clone appearing as a new live slot, the one-shot flags
--     cell (0x800260FC), and any wedge,
--   - the entry-table tag-search BP + malloc-fail BP from the special trace
--     stay armed (wedge + heap-pressure evidence).
--
-- Launch:
--   LEGAIA_POKES="$(./target/release/legaia-patcher delilas-pokes)" \
--   LEGAIA_FRAMES=18000 \
--   bash scripts/pcsx-redux/run_probe.sh --iso patched.bin \
--     --scenario sol_to_karisto_worldmap \
--     --lua scripts/pcsx-redux/autorun_delilas_gi_trace.lua
--
-- Output: delilas_gi_trace.csv  tick,mode,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE   = 0x8007B83C
local WARP_SUB    = 0x8007BA34
local COURSE_WORD = 0x8007BAC0
local TAG_SEARCH  = 0x80050E2C
local ACTOR_TABLE = 0x801C9370
local MALLOC_FAIL = 0x800178B8
-- The queue inside the Gi cave (FUN_80050d40 home, idx 39): only the
-- Divide / Spore Gas one-shots reach it (the Che & Lu queue is a separate
-- range in the main block).
local GI_QUEUE    = 0x80050DDC
-- The Gi cave's two exit tails: GNOSPEC (idx 43 - straight to the arm
-- join, no special possible) and GSTOCK (idx 45 - stock arm resumes, the
-- retail Blazing cadence may fire). Post-Divide picks must all route
-- GNOSPEC - the lockout receipt.
local GI_NOSPEC   = 0x80050DEC
local GI_STOCK    = 0x80050DF4
-- The Gi one-shot flags word (display-cave data tail): bit0 = Divide cast,
-- bit1 = Spore Gas cast; the round-0 arm zeroes it.
local GI_FLAGS    = 0x800260FC

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 18000)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local PLAYER_HP = probe.getenv_num("LEGAIA_PLAYER_HP", 0)
local GI_WEAKEN_TURN = probe.getenv_num("LEGAIA_GI_WEAKEN_TURN", 6)
local POKES_RAW = probe.getenv("LEGAIA_POKES", "")

local pokes = {}
for pair in string.gmatch(POKES_RAW, "[^,%s]+") do
    local a, v, w = string.match(pair, "([^:]+):([^:]+):?(.*)")
    if a and v then
        pokes[#pokes + 1] = { addr = tonumber(a), val = tonumber(v), byte = (w == "b") }
    end
end

local CSV = probe.csv_open(probe.out_path("delilas_gi_trace.csv"),
    "tick,mode,note")

local function reg(r, name)
    local ok, v = pcall(function() return tonumber(r.GPR.n[name]) % 0x100000000 end)
    if ok then return v end
    return 0
end

local installed = false
local last_mode = -1
local weakened = {}
local boosted = false
local gi_weakened = false
local last_search = ""
local repeat_n = 0
local search_rows = 0
local malloc_fails = 0

local function turn_counter()
    local bctx = probe.read_u32(0x8007BD24) or 0
    if bctx > 0x80000000 and bctx < 0x80200000 then
        return probe.read_u8(bctx + 0x28A) or -1
    end
    return -1
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function(ctx)
        probe.arm_breakpoint(GI_QUEUE, "Exec", 4, "gi_queue", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,GI CAST QUEUED spell=0x%02X turn=%d",
                reg(r, "v1"), turn_counter())
        end)
        probe.arm_breakpoint(GI_NOSPEC, "Exec", 4, "gi_nospec", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,GI PICK nospec seat=%d turn=%d flags=%d",
                reg(r, "s7"), turn_counter(), probe.read_u32(GI_FLAGS) or -1)
        end)
        probe.arm_breakpoint(GI_STOCK, "Exec", 4, "gi_stock", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,GI PICK stock seat=%d turn=%d flags=%d",
                reg(r, "s7"), turn_counter(), probe.read_u32(GI_FLAGS) or -1)
        end)
        -- Divide-clone spawn writer: the actor-table slot-4 cell is written
        -- by the spawn helper - its ra names the hook site for the eventual
        -- copied-action-state fix.
        probe.arm_breakpoint(ACTOR_TABLE + 4 * 4, "Write", 4, "clone_spawn", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,SLOT4 WRITE pc=0x%08X ra=0x%08X",
                (function() local ok,v = pcall(function() return tonumber(r.pc) % 0x100000000 end); return ok and v or 0 end)(),
                reg(r, "ra"))
        end)
        probe.arm_breakpoint(MALLOC_FAIL, "Exec", 4, "malloc_fail", function()
            malloc_fails = malloc_fails + 1
            if malloc_fails % 50 == 1 then
                local r = PCSX.getRegisters()
                CSV:row("0,0,malloc fail #%d size=0x%X", malloc_fails,
                    reg(r, "s1"))
            end
        end)
        probe.arm_breakpoint(TAG_SEARCH, "Exec", 4, "tag_search", function()
            if not installed then return end
            if search_rows >= 2000 then return end
            local r = PCSX.getRegisters()
            local key = string.format("tag=0x%02X count=%d ra=0x%08X",
                reg(r, "a1"), reg(r, "a2"), reg(r, "ra"))
            if key == last_search then
                repeat_n = repeat_n + 1
                return
            end
            if repeat_n > 0 then
                CSV:row("0,0,search-repeat x%d: %s", repeat_n, last_search)
                search_rows = search_rows + 1
            end
            last_search = key
            repeat_n = 0
            search_rows = search_rows + 1
            CSV:row("0,0,search %s", key)
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        if elapsed == FORCE_AT then
            for _, p in ipairs(pokes) do
                if p.byte then
                    probe.write_u8(p.addr, p.val)
                else
                    probe.write_u32(p.addr, p.val)
                end
            end
            probe.write_u32(WARP_SUB, 5)
            probe.write_u32(COURSE_WORD, 0)
            probe.write_u16(GAME_MODE, 0x18)
            installed = true
            CSV:row("%d,0x%X,warp forced (%d pokes)", elapsed, mode, #pokes)
            return
        end
        local word = probe.read_u32(COURSE_WORD) or 0
        if mode ~= last_mode then
            CSV:row("%d,0x%X,mode-change word=0x%X", elapsed, mode, word)
            if mode == 0x15 then
                weakened = {}
                boosted = false
                gi_weakened = false
            end
            last_mode = mode
        end
        if mode == 0x15 and word == 0x131 then
            -- Round 0: weaken each sibling on its own first live tick.
            for slot = 3, 5 do
                if not weakened[slot] then
                    local a = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                    if a > 0x80000000 and a < 0x80200000 then
                        if (probe.read_u16(a + 0x14C) or 0) > 1 then
                            probe.write_u16(a + 0x14C, 1)
                            weakened[slot] = true
                            CSV:row("%d,0x%X,slot %d weakened", elapsed, mode, slot)
                        end
                    end
                end
            end
        end
        if mode == 0x15 and word == 0x132 and not gi_weakened and GI_WEAKEN_TURN ~= 0
            and turn_counter() >= GI_WEAKEN_TURN then
            -- Drop Gi below the half-HP Divide gate (enemy raw writes are
            -- safe; only party actors carry the delta-driven mirror trap).
            local a = probe.read_u32(ACTOR_TABLE + 3 * 4) or 0
            if a > 0x80000000 and a < 0x80200000 then
                local max = probe.read_u16(a + 0x14E) or 0
                if max > 0 and (probe.read_u16(a + 0x14C) or 0) > 0 then
                    probe.write_u16(a + 0x14C, math.floor(max / 4))
                    gi_weakened = true
                    CSV:row("%d,0x%X,gi weakened to %d/%d at turn %d", elapsed,
                        mode, math.floor(max / 4), max, turn_counter())
                end
            end
        end
        if mode == 0x15 and word == 0x132 and not boosted and PLAYER_HP ~= 0 then
            -- Wedge-shape experiments only (see header) - off by default.
            local a = probe.read_u32(ACTOR_TABLE) or 0
            if a > 0x80000000 and a < 0x80200000 then
                if (probe.read_u16(a + 0x14C) or 0) > 0 then
                    probe.write_u16(a + 0x14C, PLAYER_HP)
                    boosted = true
                    CSV:row("%d,0x%X,player hp boosted to %d", elapsed, mode,
                        PLAYER_HP)
                end
            end
        end
        if installed then
            -- UP-then-Cross: the intermission continue-menu's first option
            -- keeps the course going (a bare Cross mash picks "leave").
            local phase = elapsed % 40
            if phase == 0 then
                pad.force(pad.BTN.UP)
            elseif phase == 4 then
                pad.release(pad.BTN.UP)
            elseif phase == 8 then
                pad.force(pad.BTN.CROSS)
            elseif phase == 14 then
                pad.release(pad.BTN.CROSS)
            end
            if elapsed % 60 == 0 and mode == 0x15 then
                local cells = {}
                for slot = 0, 5 do
                    local a = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                    if a > 0x80000000 and a < 0x80200000 then
                        cells[#cells + 1] = string.format(
                            "s%d anim=%02X/%02X q=%X tgt=%X act=%02X hp=%d/%d",
                            slot,
                            probe.read_u8(a + 0x1D9) or 0,
                            probe.read_u8(a + 0x1DA) or 0,
                            probe.read_u8(a + 0x1DE) or 0,
                            probe.read_u8(a + 0x1DD) or 0,
                            probe.read_u8(a + 0x1DF) or 0,
                            probe.read_u16(a + 0x14C) or 0,
                            probe.read_u16(a + 0x172) or 0)
                    end
                end
                -- ctx[7] = action-state cursor, ctx[0x13] = active actor.
                local bctx = probe.read_u32(0x8007BD24) or 0
                local st, cur = -1, -1
                if bctx > 0x80000000 and bctx < 0x80200000 then
                    st = probe.read_u8(bctx + 7) or -1
                    cur = probe.read_u8(bctx + 0x13) or -1
                end
                CSV:row("%d,0x%X,word=0x%X turn=%d st=0x%02X cur=%d flags=%d actors %s",
                    elapsed, mode, word, turn_counter(),
                    st, cur, probe.read_u32(GI_FLAGS) or -1,
                    table.concat(cells, " | "))
            end
        end
    end,
    on_done = function()
        if repeat_n > 0 then
            CSV:row("0,0,search-repeat x%d: %s", repeat_n, last_search)
        end
        CSV:row("0,0x%X,done malloc_fails=%d", last_mode, malloc_fails)
        pcall(function()
            local sstate = require("probe.sstate")
            sstate.save(probe.out_path("gi_trace_end.sstate"))
        end)
        CSV:close()
    end,
})
