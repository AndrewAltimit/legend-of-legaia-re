-- autorun_delilas_reward_trace.lua
--
-- Drive the Delilas dome course to a WIN and instrument the settlement:
-- monsters are weakened to 1 HP once at each battle's start (a one-shot
-- write before any action - blind mid-battle HP writes wedge the settle
-- bookkeeping), Cross-mash clears both rounds, and breakpoints on the
-- reward detour pin what the payout path actually does:
--   - 0x801D1118 (REWARD_HOOK): log v0 (table slot), v1 (counter loaded),
--     a1 (round), plus the course/round globals,
--   - 0x8007AE84 (cave reward routine): detour-entry receipt,
--   - 0x801D1128 (post-store): the stored counter value.
-- The winnings counter 0x80084440 (game-state window +0x300) is logged
-- every 60 ticks - seed it high via LEGAIA_POKES to reproduce the
-- "cheated balance zeroed" report.
--
-- Output: delilas_reward_trace.csv  tick,mode,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE   = 0x8007B83C
local WARP_SUB    = 0x8007BA34
local COURSE_WORD = 0x8007BAC0
local ACTOR_TABLE = 0x801C9370
local COUNTER     = 0x80084440
local COURSE_G    = 0x801D1A90
local ROUND_G     = 0x801D1A94
local REWARD_CAVE = 0x8007AE8C
-- Round-end routing / settlement state (arena-overlay globals - RAM reads
-- only, no BPs: the battle overlay aliases the VA band and BPs there fire
-- constantly on the wrong code).
local MENU_RESIDUE = 0x80084448 -- koin1 menu-selection residue (4 = "quit" arm)
local GRANT_CAVE   = 0x800352EC -- custom-items grant routine (AI-block tail)
local BAG          = 0x80085958 -- 2 bytes/slot [id, count]
local REWARD_ITEMS = { 0xB9, 0x12, 0x1A }
local WIN_LATCH    = 0x801D1ADC -- settle pays only while this is set
local RAN_AWAY     = 0x801D1A74 -- set by the quit arm -> counter zeroed
local BATTLE_RESULT = 0x80083D60 -- bit 0x80 = survived
-- Dialog pager text pointer (field overlay 0897; FUN_80039b7c writes
-- `_DAT_801f3538 = *(actor+0x90) + (short)*(actor+0x9e)` before paging).
-- Sampling it whenever it changes yields the exact narration strings the
-- post-contest award ceremony pages - the receipt that the win path shows
-- the reward box (and not a cut-short flow). Only meaningful in field
-- modes: in battle the VA band belongs to the battle overlay.
local PAGER_TEXT = 0x801F3538
-- The decompressed-MAN heap buffer pointer (`_DAT_8007B898`): the scene's
-- MAN lives at `*MAN_BUF_PTR`, so a *data read-watch* at buffer + a known
-- MAN offset is a byte-exact "this text was consumed" receipt, immune to
-- every display-path unknown. The offsets are the patched koin1 MAN's
-- narration texts (LEGAIA_NARR_OFFS env: label:off,label:off,... - the
-- caller computes them from its own patched image; defaults match the
-- seed-1 --delilas-challenge --custom-items build).
local MAN_BUF_PTR = 0x8007B898
local NARR_OFFS_RAW = probe.getenv("LEGAIA_NARR_OFFS",
    "box1_retail:0x4D39,tokens_retail:0x4D78,box1_copy:0x4EA8,item_text:0x4EE7")

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 12000)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local POKES_RAW = probe.getenv("LEGAIA_POKES", "")
-- Skip the enemy weaken (round 0 runs at full HP): the long full-HP fight
-- is what exercises the effect-alloc bursts behind the PRG ERR print.
local NO_WEAKEN = probe.getenv_num("LEGAIA_NO_WEAKEN", 0)
-- The dev reporter's PRG ERR print jal (only reachable if the print-gate
-- patch is absent/broken) and the malloc-failure branch (proves a run
-- actually produced the bursts, so a silent print BP is non-vacuous).
local PRGERR_PRINT_JAL = 0x800164E0
local MALLOC_FAIL      = 0x800178B8

local pokes = {}
for pair in string.gmatch(POKES_RAW, "[^,%s]+") do
    local a, v, w = string.match(pair, "([^:]+):([^:]+):?(.*)")
    if a and v then
        pokes[#pokes + 1] = { addr = tonumber(a), val = tonumber(v), byte = (w == "b") }
    end
end

local CSV = probe.csv_open(probe.out_path("delilas_reward_trace.csv"),
    "tick,mode,note")

local function reg(r, name)
    local ok, v = pcall(function() return tonumber(r.GPR.n[name]) % 0x100000000 end)
    if ok then return v end
    return 0
end

local installed = false
local last_mode = -1
local weakened = {}
local malloc_fails = 0
local last_pager = 0
local last_pstate, last_plines = -2, -2
local shots = 0
local narr_armed = false
local narr_hits = {}
local now_tick = 0

-- Framebuffer screenshot to an explicit sibling of LEGAIA_OUT (out_path
-- returns LEGAIA_OUT verbatim for ANY name, so writing through it would
-- clobber the CSV). Dialog boxes wait for input across many vsyncs, so the
-- displayed-buffer lag trap doesn't bite here.
local function shot(name)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or not ss then return end
    local base = probe.getenv("LEGAIA_OUT", "")
    if base == "" then base = probe.out_path("delilas_reward_trace.csv") end
    local h = io.open(base .. "." .. name .. ".screen", "wb")
    if not h then return end
    h:write(tostring(ss.data)); h:close()
    local m = io.open(base .. "." .. name .. ".screen.meta", "w")
    if m then
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            tonumber(ss.width), tonumber(ss.height),
            (tonumber(ss.bpp) or 0) > 16 and 24 or 16))
        m:close()
    end
    shots = shots + 1
end

-- Read up to `n` bytes at `ptr` as printable ASCII (MES markup bytes are
-- rendered as {XX}); returns nil when the first byte isn't text-like.
local function pager_string(ptr, n)
    local first = probe.read_u8(ptr)
    if not first or (first < 0x20 and first ~= 0x1F) or first == 0x7F then
        return nil
    end
    local out = {}
    for i = 0, n - 1 do
        local b = probe.read_u8(ptr + i)
        if not b or b == 0 then break end
        if b >= 0x20 and b < 0x7F then
            out[#out + 1] = string.char(b)
        else
            out[#out + 1] = string.format("{%02X}", b)
        end
    end
    return table.concat(out)
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function(ctx)
        -- NB: breakpoints on arena-overlay VAs (0x801Dxxxx) are useless here -
        -- the battle overlay aliases the band and fires them constantly. The
        -- SCUS cave (0x8007xxxx) is always-resident and unambiguous.
        probe.arm_breakpoint(REWARD_CAVE, "Exec", 4, "reward_cave", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,CAVE reward routine entered course_g=%d round_g=%d",
                probe.read_u32(COURSE_G) or -1, probe.read_u32(ROUND_G) or -1)
        end)
        -- The winnings-display override over FUN_800260DC (SCUS - no
        -- VA aliasing): entry receipt + whether the course-3 arm was taken
        -- (v1 = course<<6 at entry; 0xC0 = course 3).
        probe.arm_breakpoint(0x800260DC, "Exec", 4, "display_routine", function()
            local r = PCSX.getRegisters()
            local v1 = 0
            pcall(function() v1 = tonumber(r.GPR.n.v1) % 0x100000000 end)
            CSV:row("0,0,DISPLAY routine entered course_shifted=0x%X", v1)
        end)
        -- Custom-items grant (fires on every settle; gives only when the
        -- settling course is 3 - the receipt logs the course global).
        probe.arm_breakpoint(GRANT_CAVE, "Exec", 4, "grant_cave", function()
            CSV:row("0,0,GRANT routine entered course_g=%d", probe.read_u32(COURSE_G) or -1)
        end)
        -- Dialog text receipt: the actor-dialog SM's pager-pointer store
        -- (`sw v0,0x3538(v1)` at 0x8003A000 in FUN_80039B7C - SCUS band, so
        -- the BP is alias-free). v0 = script buffer + cursor = the text the
        -- pager is about to page; the poll-based watch below misses it (the
        -- cell is transient within the frame).
        probe.arm_breakpoint(0x8003A000, "Exec", 4, "pager_set", function()
            local r = PCSX.getRegisters()
            local v0 = 0
            pcall(function() v0 = tonumber(r.GPR.n.v0) % 0x100000000 end)
            local s = v0 > 0x80000000 and v0 < 0x80200000 and pager_string(v0, 72) or nil
            CSV:row("0,0,pager \"%s\" (ptr=0x%08X)", s and (s:gsub(",", ";")) or "?", v0)
        end)
        probe.arm_breakpoint(PRGERR_PRINT_JAL, "Exec", 4, "prgerr_print", function()
            CSV:row("0,0,PRGERR PRINT reached (gate patch absent/broken) accum=%d",
                probe.read_u32(0x8007B828) or -1)
        end)
        probe.arm_breakpoint(MALLOC_FAIL, "Exec", 4, "malloc_fail", function()
            malloc_fails = malloc_fails + 1
            if malloc_fails % 50 == 1 then
                CSV:row("0,0,malloc fail #%d (accum=%d)", malloc_fails,
                    probe.read_u32(0x8007B828) or -1)
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        now_tick = elapsed
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
        -- Post-course ceremony screenshots: once the course is over and the
        -- game is back in a field mode, sample the framebuffer often enough
        -- to catch every dialog box the mash pages through (a box waits for
        -- input across many vsyncs). NB `takeScreenShot()` returns all-zero
        -- under the hardware-GPU profile - the pager trace below is the
        -- receipt that works everywhere; screenshots stay opt-in.
        if installed and probe.getenv_num("LEGAIA_SHOTS", 0) == 1
            and elapsed > 6000 and (mode == 0x2 or mode == 0x3) and elapsed % 50 == 0 then
            shot(string.format("t%05d", elapsed))
        end
        -- Arm the narration read-watches once the post-course koin1 scene
        -- is loaded (the MAN buffer is fresh then). One hit per site is
        -- enough - the callback logs the first read and the running set.
        if installed and not narr_armed and elapsed > 6000 and (mode == 0x2 or mode == 0x3) then
            local base = probe.read_u32(MAN_BUF_PTR) or 0
            if base > 0x80000000 and base < 0x80200000 then
                narr_armed = true
                -- A scene-entry pass linearly sweeps the whole record region
                -- once (~150 ticks after the load, every site in one burst),
                -- so a once-latch is blind: log up to 5 timestamped hits per
                -- site and let the timeline separate the sweep (all sites,
                -- one instant) from genuine paging (copy/item sites again,
                -- spread across the ceremony; retail sites silent).
                for pair in string.gmatch(NARR_OFFS_RAW, "[^,%s]+") do
                    local label, off = string.match(pair, "([^:]+):(.+)")
                    if label and off then
                        local addr = base + tonumber(off)
                        probe.arm_breakpoint(addr, "Read", 2, "narr_" .. label, function()
                            local h = narr_hits[label]
                            if not h then
                                h = {}
                                narr_hits[label] = h
                            end
                            local bucket = math.floor(now_tick / 100) * 100
                            h[bucket] = (h[bucket] or 0) + 1
                        end)
                    end
                end
                CSV:row("%d,0x%X,narr watches armed base=0x%08X", elapsed, mode, base)
            end
        end
        -- Dialog pager trace: `_DAT_801f2734` (pager state) + `_DAT_801f2740`
        -- (row count of the box being laid out). Logged on every change while
        -- in a field mode: the award ceremony's win path reads as a 3-row box
        -- (good fight / well done / enter again) followed by ANOTHER 3-row
        -- box (the reward rows), where the retail tokens box is 2 rows and
        -- the cut-short failure shape is a single activation then silence.
        if mode == 0x2 or mode == 0x3 then
            local ps = probe.read_u32(0x801F2734) or -1
            local pl = probe.read_u32(0x801F2740) or -1
            if ps ~= last_pstate or pl ~= last_plines then
                CSV:row("%d,0x%X,pagerstate %d lines %d", elapsed, mode, ps, pl)
                last_pstate, last_plines = ps, pl
            end
        end
        -- Dialog pager watch (field modes only - the VA aliases battle
        -- overlay memory in mode 0x15): log each new text the pager shows.
        if mode ~= 0x15 then
            local p = probe.read_u32(PAGER_TEXT) or 0
            if p ~= last_pager then
                last_pager = p
                if p > 0x80000000 and p < 0x80200000 then
                    local s = pager_string(p, 72)
                    if s then
                        CSV:row("%d,0x%X,pager \"%s\"", elapsed, mode, (s:gsub(",", ";")))
                    end
                end
            end
        end
        if mode ~= last_mode then
            CSV:row("%d,0x%X,mode-change word=0x%X counter=%d display=%d", elapsed, mode,
                probe.read_u32(COURSE_WORD) or 0, probe.read_u32(COUNTER) or -1,
                probe.read_u32(0x801D1AAC) or -1)
            CSV:row("%d,0x%X,routing residue=%d win_latch=%d ran_away=%d result=0x%02X",
                elapsed, mode,
                probe.read_u32(MENU_RESIDUE) or -1,
                probe.read_u32(WIN_LATCH) or -1,
                probe.read_u32(RAN_AWAY) or -1,
                probe.read_u8(BATTLE_RESULT) or 0)
            if mode == 0x15 then
                weakened = {}
            end
            last_mode = mode
        end
        -- Battle HP trace: discriminates "mash lost the round" from "round
        -- won but the intermission routed out" - the two exits the
        -- mode/counter rows cannot tell apart.
        if mode == 0x15 and elapsed % 120 == 0 then
            local hps = {}
            for slot = 0, 5 do
                local a = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                if a > 0x80000000 and a < 0x80200000 then
                    hps[#hps + 1] = string.format("%d", probe.read_u16(a + 0x14C) or -1)
                else
                    hps[#hps + 1] = "-"
                end
            end
            CSV:row("%d,0x%X,hp %s", elapsed, mode, table.concat(hps, "/"))
        end
        -- Weaken each enemy on the FIRST tick its own HP reads live,
        -- one-shot per slot per battle: a fixed "entry+20" delay fires
        -- before battle setup stages any actor (every HP still 0 -> the
        -- >1 guard skips all slots), and the seats stage at DIFFERENT
        -- ticks - a single all-slots pass on the first live seat left the
        -- late-staging sibling at full HP. Per-slot first-live is still
        -- before any action on that actor, which is what the
        -- never-write-HP-mid-battle trap is about.
        if mode == 0x15 and NO_WEAKEN == 0 then
            for slot = 3, 5 do
                if not weakened[slot] then
                    local a = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                    if a > 0x80000000 and a < 0x80200000 then
                        if (probe.read_u16(a + 0x14C) or 0) > 1 then
                            probe.write_u16(a + 0x14C, 1)
                            weakened[slot] = true
                            CSV:row("%d,0x%X,slot %d weakened to 1 hp", elapsed, mode, slot)
                        end
                    end
                end
            end
        end
        if installed then
            -- Mash Up then Cross: the intermission continue-menu's first
            -- option is the one that keeps the course going, and a bare
            -- Cross mash has been observed selecting "leave".
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
            if elapsed % 60 == 0 then
                CSV:row("%d,0x%X,counter=%d word=0x%X", elapsed, mode,
                    probe.read_u32(COUNTER) or -1,
                    probe.read_u32(COURSE_WORD) or 0)
            end
        end
    end,
    on_done = function()
        local counts = {}
        for _, want in ipairs(REWARD_ITEMS) do
            local n = 0
            for slot = 0, 0x1FF do
                if (probe.read_u8(BAG + slot * 2) or 0) == want then
                    n = n + (probe.read_u8(BAG + slot * 2 + 1) or 0)
                end
            end
            counts[#counts + 1] = string.format("0x%02X=%d", want, n)
        end
        local hit_list = {}
        for k, h in pairs(narr_hits) do
            local total = 0
            local buckets = {}
            for b, n in pairs(h) do
                total = total + n
                buckets[#buckets + 1] = b
            end
            table.sort(buckets)
            local parts = {}
            for _, b in ipairs(buckets) do
                parts[#parts + 1] = string.format("%d:%d", b, h[b])
            end
            hit_list[#hit_list + 1] = string.format("%s=%d{%s}", k, total,
                table.concat(parts, " "))
        end
        table.sort(hit_list)
        CSV:row("0,0x%X,done counter=%d bag %s narr_reads=[%s]", last_mode,
            probe.read_u32(COUNTER) or -1, table.concat(counts, " "),
            table.concat(hit_list, " "))
        pcall(function()
            local sstate = require("probe.sstate")
            -- NB: probe.out_path returns LEGAIA_OUT VERBATIM when set, so
            -- naming a second output through it would overwrite the CSV
            -- (that exact clobber ate one run's trace). Derive a sibling.
            local out = probe.getenv("LEGAIA_OUT", "")
            local path = out ~= "" and (out .. ".end.sstate")
                or probe.out_path("reward_trace_end.sstate")
            sstate.save(path)
        end)
        CSV:close()
    end,
})
