-- autorun_footstep_cue.lua
--
-- Answers "which SFX cue id does retail play when the player walks in a
-- field scene?" as a CONTRAST, not as a single observation: run the same
-- save state twice for the same number of vsyncs, once standing still and
-- once with the D-pad held, and diff the cue traffic. A cue id that only
-- appears in the walking run, at a cadence, is a footstep. An id present in
-- both is ambient or incidental. NO id in either is the negative result -
-- retail plays nothing there - and that is a real answer, not a failure.
--
-- Every cue path documented in docs/formats/sfx-table.md is watched at once,
-- so the answer does not depend on guessing which one a footstep would use:
--
--   * FUN_80035B50(id)  ring push  - writes DAT_8007B6D8[cursor] + timer 0
--   * FUN_80035BD0(id)  ring overwrite of the current slot
--   * FUN_8004FCC8(id)  cue / voice dispatcher (id >= 0x100 -> CD-XA clip)
--   * FUN_800250D4(id, voice)  per-actor SFX trigger (bypasses the ring)
--   * FUN_80065034(...)         the SpuSetVoiceAttr analogue the ring
--                               drainer FUN_80016B6C calls - proves a cue
--                               actually programmed a voice
--   * Write watches on the ring itself (DAT_8007B6D8[0..3], i16) catch any
--     producer that skips all of the above and stores the id directly.
--
-- It also watches the two bytes FUN_80018DB0 (the per-frame field cadence
-- this repo's FootstepCadence port mirrors) writes at DAT_800915DA/DB. Those
-- are the libpad actuator table registered by the pad init FUN_8001D230
-- (`FUN_8006CE30(port, table, 2)` for port 0 at +2, port 1 at +0x42, inside
-- the 0x80-byte block zeroed at 0x800915D8), i.e. DualShock RUMBLE - so their
-- 0->1 edges are retail's per-step beat, and counting them gives a step count
-- to hold the cue count against. A run with many step edges and zero cue
-- writes is the evidenced negative.
--
-- Env vars:
--   LEGAIA_SSTATE   save state (must be a walkable FIELD scene; the probe
--                   logs mode/scene/player-XZ so a wrong state is visible)
--   LEGAIA_FRAMES   capture vsyncs (default 900)
--   LEGAIA_WALK     1 = inject D-pad, 0 = stand still (default 1)
--   LEGAIA_WALK_DIRS   comma list of BTN names cycled while walking
--                      (default "UP,RIGHT,DOWN,LEFT"); never SELECT/START,
--                      which open menus
--   LEGAIA_WALK_SEG    vsyncs per direction segment (default 90)
--   LEGAIA_WALK_START  vsyncs to wait after the state load before the first
--                      press (default 60; lets the scene settle)
--
-- Run both halves of the contrast (the kill timeout is mandatory - PCSX.quit
-- does not reliably exit the process):
--
--   for w in 0 1; do
--     LEGAIA_WALK=$w LEGAIA_FRAMES=900 \
--     LEGAIA_OUT_DIR=captures/footstep_cue/walk$w \
--     timeout --kill-after=20s 900s \
--       bash scripts/pcsx-redux/run_probe.sh \
--         --sstate <a walkable field .sstate> \
--         --lua scripts/pcsx-redux/autorun_footstep_cue.lua
--   done
--
-- Then diff the two `cue` id sets. Output files per run:
--   footstep_cue.csv       every cue/producer/ring/actuator event
--   footstep_cue.hits.txt  probe.run's per-descriptor hit snapshot

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 900)
local WALK        = probe.getenv_num("LEGAIA_WALK", 1) ~= 0
local WALK_DIRS   = probe.getenv("LEGAIA_WALK_DIRS", "UP,RIGHT,DOWN,LEFT")
local WALK_SEG    = math.max(1, probe.getenv_num("LEGAIA_WALK_SEG", 90))
local WALK_START  = probe.getenv_num("LEGAIA_WALK_START", 60)
local OUT_PATH    = probe.out_path("footstep_cue.csv")

-- ---------------------------------------------------------------- addresses
local RING_BASE   = 0x8007B6D8  -- DAT_8007B6D8[4], i16 cue ids
local RING_TIMER  = 0x8007C338  -- DAT_8007C338[4], u32 countdown in vsyncs
local ACT_TABLE   = 0x800915DA  -- libpad port-0 actuator pair (rumble)
local STEP_TIMER  = 0x8007B8A4  -- _DAT_8007B8A4, the cadence countdown
local STEP_FLAG   = 0x8007B79C  -- DAT_8007B79C, cadence branch selector
local GAME_MODE   = 0x8007B83C
local SCENE_NAME  = 0x8007050C
local PLAYER_PTR  = 0x8007C364
local PLAYER_X    = 0x14        -- i16
local PLAYER_Z    = 0x18        -- i16

local FN_RING_PUSH  = 0x80035B50
local FN_RING_OVER  = 0x80035BD0
local FN_DISPATCH   = 0x8004FCC8
local FN_ACTOR_SFX  = 0x800250D4
local FN_VOICE_ATTR = 0x80065034
local FN_CADENCE    = 0x80018DB0

-- ---------------------------------------------------------------- utilities
local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then return v - 0x10000 end
    return v
end

-- The debug hook runs BEFORE the store, so memory still holds the PRE value.
-- Decode the store instruction at PC instead and read its source register,
-- which is the value about to land. Returns nil for a non-store PC.
local function stored_value()
    local r    = PCSX.getRegisters()
    local pc   = n32(r.pc)
    local insn = probe.read_u32(pc)
    if insn == nil then return nil, pc end
    insn = n32(insn)
    local op = bit.rshift(insn, 26)
    local rt = bit.band(bit.rshift(insn, 16), 0x1F)
    local v  = n32(r.GPR.r[rt])
    if op == 0x28 then return bit.band(v, 0xFF), pc      -- sb
    elseif op == 0x29 then return bit.band(v, 0xFFFF), pc -- sh
    elseif op == 0x2B then return v, pc                   -- sw
    end
    return nil, pc
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function player_xz()
    local p = probe.read_u32(PLAYER_PTR)
    if p == nil or not probe.in_ram(n32(p)) then return nil end
    local x = probe.read_u16(n32(p) + PLAYER_X)
    local z = probe.read_u16(n32(p) + PLAYER_Z)
    if x == nil or z == nil then return nil end
    return s16(x), s16(z)
end

local function ring_hex()
    local out = {}
    for i = 0, 3 do
        out[#out + 1] = string.format("%d", s16(probe.read_u16(RING_BASE + i * 2) or 0))
    end
    return table.concat(out, " ")
end

-- ---------------------------------------------------------------- state
local csv = probe.csv_open(OUT_PATH,
    "vsync,kind,id,arg1,pc,ra,note")
local g_vsync   = 0
local counts    = {}           -- kind -> hits
local cue_ids   = {}           -- id -> hits, across every producer
local step_edges = 0           -- 0->1 transitions of the rumble trigger byte
local last_act  = {}           -- actuator byte address -> last value
local moved     = false
local start_xz  = nil
local last_xz   = nil

local function bump(kind)
    counts[kind] = (counts[kind] or 0) + 1
end

local function log_cue(kind, id, arg1, note)
    bump(kind)
    if id ~= nil then cue_ids[id] = (cue_ids[id] or 0) + 1 end
    local r  = PCSX.getRegisters()
    local pc = n32(r.pc)
    local ra = n32(r.GPR.n.ra)
    -- Log first so the datum survives even if the CSV sink is gone.
    PCSX.log(string.format("[foot] v=%d %s id=%s arg1=%s pc=0x%08X ra=0x%08X %s",
        g_vsync, kind, id and string.format("0x%02X", id) or "-",
        arg1 and string.format("0x%X", arg1) or "-", pc, ra, note or ""))
    if csv then
        csv:row("%d,%s,%s,%s,0x%08X,0x%08X,%s", g_vsync, kind,
            id and string.format("0x%02X", id) or "",
            arg1 and string.format("0x%X", arg1) or "",
            pc, ra, note or "")
    end
end

-- ---------------------------------------------------------------- pad walk
local dirs = {}
for name in string.gmatch(WALK_DIRS, "[^,]+") do
    local up = string.upper((name:gsub("%s", "")))
    if probe.BTN[up] ~= nil and up ~= "SELECT" and up ~= "START" then
        dirs[#dirs + 1] = { name = up, btn = probe.BTN[up] }
    end
end

local held = nil
local function walk_tick(elapsed)
    if not WALK or #dirs == 0 then return end
    if elapsed < WALK_START then return end
    local seg  = math.floor((elapsed - WALK_START) / WALK_SEG)
    local want = dirs[(seg % #dirs) + 1]
    if held ~= nil and held.btn == want.btn then return end
    if held ~= nil then probe.pad_release(held.btn) end
    probe.pad_force(want.btn)
    held = want
    PCSX.log(string.format("[foot] v=%d hold %s", elapsed, want.name))
end

-- ---------------------------------------------------------------- run
probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.write_manifest("autorun_footstep_cue.lua", {
            sstate = SSTATE_PATH, frames = FRAMES,
            walk = tostring(WALK), walk_dirs = WALK_DIRS,
            walk_seg = WALK_SEG, walk_start = WALK_START,
        })
        local descs = {}
        local function exec(addr, name, cb)
            local d = { addr = addr, name = name, hits_ref = { n = 0 } }
            probe.arm_breakpoint(addr, "Exec", 4, name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                cb()
            end)
            descs[#descs + 1] = d
        end

        exec(FN_RING_PUSH, "ring_push", function()
            local r = PCSX.getRegisters()
            log_cue("ring_push", bit.band(n32(r.GPR.n.a0), 0xFFFF), nil, "FUN_80035B50")
        end)
        exec(FN_RING_OVER, "ring_over", function()
            local r = PCSX.getRegisters()
            log_cue("ring_over", bit.band(n32(r.GPR.n.a0), 0xFFFF), nil, "FUN_80035BD0")
        end)
        exec(FN_DISPATCH, "dispatch", function()
            local r = PCSX.getRegisters()
            log_cue("dispatch", n32(r.GPR.n.a0), nil, "FUN_8004FCC8")
        end)
        exec(FN_ACTOR_SFX, "actor_sfx", function()
            local r = PCSX.getRegisters()
            log_cue("actor_sfx", n32(r.GPR.n.a0), n32(r.GPR.n.a1), "FUN_800250D4")
        end)
        exec(FN_VOICE_ATTR, "voice_attr", function()
            local r = PCSX.getRegisters()
            log_cue("voice_attr", nil, n32(r.GPR.n.a0),
                string.format("a1=0x%X a2=0x%X", n32(r.GPR.n.a1), n32(r.GPR.n.a2)))
        end)
        -- Cadence tick: hits once per field frame. Counted, never logged
        -- per-hit (that would be one CSV row per vsync).
        local cad = { addr = FN_CADENCE, name = "cadence_tick", hits_ref = { n = 0 } }
        probe.arm_breakpoint(FN_CADENCE, "Exec", 4, "cadence_tick", function()
            cad.hits_ref.n = cad.hits_ref.n + 1
        end)
        descs[#descs + 1] = cad

        -- Ring stores from ANY producer, including ones that call none of
        -- the functions above. Width 2 per slot: a width-4 watch would miss
        -- a halfword store to the odd slot of its pair.
        for i = 0, 3 do
            local addr = RING_BASE + i * 2
            local name = string.format("ring[%d]", i)
            local d = { addr = addr, name = name, hits_ref = { n = 0 } }
            probe.arm_breakpoint(addr, "Write", 2, name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                local v, pc = stored_value()
                local r = PCSX.getRegisters()
                bump("ring_store")
                if v ~= nil and v ~= 0xFFFF then cue_ids[v] = (cue_ids[v] or 0) + 1 end
                if csv then
                    csv:row("%d,ring_store,%s,%d,0x%08X,0x%08X,slot%d timer=%s",
                        g_vsync,
                        v and string.format("0x%02X", v) or "",
                        i, pc, n32(r.GPR.n.ra), i,
                        tostring(probe.read_u32(RING_TIMER + i * 4)))
                end
            end)
            descs[#descs + 1] = d
        end

        -- The rumble actuator pair FUN_80018DB0 arms per step. Logged only on
        -- a value change, and 0->1 edges of the trigger byte are counted as
        -- retail's step beat.
        local act = { addr = ACT_TABLE, name = "act_table", hits_ref = { n = 0 } }
        probe.arm_breakpoint(ACT_TABLE, "Write", 2, "act_table", function()
            act.hits_ref.n = act.hits_ref.n + 1
            local v, pc = stored_value()
            if v == nil then return end
            local target = 0
            do
                local insn = n32(probe.read_u32(pc) or 0)
                local imm  = bit.band(insn, 0xFFFF)
                if imm >= 0x8000 then imm = imm - 0x10000 end
                local base = n32(PCSX.getRegisters().GPR.r[bit.band(bit.rshift(insn, 21), 0x1F)])
                target = n32(base + imm)
            end
            local prev = last_act[target]
            if prev == v then return end
            last_act[target] = v
            if (prev or 0) == 0 and v ~= 0 then step_edges = step_edges + 1 end
            bump("act_store")
            if csv then
                csv:row("%d,act_store,0x%02X,0x%08X,0x%08X,,%s",
                    g_vsync, v, target, pc,
                    string.format("prev=%s edges=%d", tostring(prev), step_edges))
            end
        end)
        descs[#descs + 1] = act

        PCSX.log(string.format(
            "[foot] armed: walk=%s dirs=%s seg=%d frames=%d",
            tostring(WALK), WALK_DIRS, WALK_SEG, FRAMES))
        return descs
    end,

    on_capture = function(_ctx, elapsed)
        g_vsync = elapsed
        walk_tick(elapsed)
        local x, z = player_xz()
        if x ~= nil then
            if start_xz == nil then start_xz = { x, z } end
            if last_xz ~= nil and (x ~= last_xz[1] or z ~= last_xz[2]) then
                moved = true
            end
            last_xz = { x, z }
        end
        -- Periodic context row: proves the run was in a field scene and
        -- (for the walking half) that the player actually moved.
        if elapsed % 60 == 0 and csv then
            csv:row("%d,ctx,,,,,mode=0x%02X scene=%s pos=%s ring=[%s] "
                .. "step_timer=%s foot_flag=%s steps=%d",
                elapsed, probe.read_u8(GAME_MODE) or 0xFF, scene_name(),
                x and string.format("(%d;%d)", x, z) or "n/a",
                ring_hex(),
                tostring(probe.read_u32(STEP_TIMER)),
                tostring(probe.read_u32(STEP_FLAG)),
                step_edges)
        end
    end,

    on_done = function(_ctx, descs)
        if held ~= nil then probe.pad_release(held.btn) end
        for _, d in ipairs(dirs) do probe.pad_release(d.btn) end
        if csv then csv:close() end

        PCSX.log("=== footstep-cue probe ===")
        PCSX.log(string.format("  walk=%s  moved=%s  start=%s  end=%s",
            tostring(WALK), tostring(moved),
            start_xz and string.format("(%d,%d)", start_xz[1], start_xz[2]) or "n/a",
            last_xz and string.format("(%d,%d)", last_xz[1], last_xz[2]) or "n/a"))
        PCSX.log(string.format("  scene=%s mode=0x%02X",
            scene_name(), probe.read_u8(GAME_MODE) or 0xFF))
        PCSX.log(string.format("  cadence step edges (rumble 0->1) = %d", step_edges))
        for _, d in ipairs(descs or {}) do
            PCSX.log(string.format("  hits %-12s 0x%08X  %d",
                d.name, d.addr, (d.hits_ref and d.hits_ref.n) or 0))
        end
        local kinds = {}
        for k in pairs(counts) do kinds[#kinds + 1] = k end
        table.sort(kinds)
        for _, k in ipairs(kinds) do
            PCSX.log(string.format("  events %-12s %d", k, counts[k]))
        end
        local ids = {}
        for id in pairs(cue_ids) do ids[#ids + 1] = id end
        table.sort(ids)
        if #ids == 0 then
            PCSX.log("  CUE IDS: none - no producer wrote the SFX ring in this run")
        else
            for _, id in ipairs(ids) do
                PCSX.log(string.format("  cue 0x%02X  x%d", id, cue_ids[id]))
            end
        end
        PCSX.log("=== end ===")
    end,
})
