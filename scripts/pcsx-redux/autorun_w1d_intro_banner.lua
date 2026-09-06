-- autorun_w1d_intro_banner.lua
--
-- Who raises the battle-INTRO enemy-name banner, and out of what.
--
-- The battle HUD's retained text actors live in one doubly-linked list
-- whose anchor is gp[+0x148] = 0x8007B460 (gp = 0x8007B318); nodes are
-- 0x34 bytes: +0x00 next, +0x04 prev, +0x08 id u16, +0x0A x i16,
-- +0x0C y i16, +0x0E w i16, +0x10 h i16, +0x18 string pointer,
-- +0x1C class byte, +0x1D kind byte (the widget-class index). Every node
-- is created by FUN_8003541C(id, class, str, x, y, w, h, kind).
--
-- Two callers can create one: FUN_801D8DE8(record, mode), which forwards
-- a screen-element placement record (0x80076C10 + id*0x18) field for
-- field, and FUN_801D9D3C, the battle-intro composer, which passes
-- immediates. This probe logs every FUN_8003541C call with its $ra, so
-- each intro sprite is attributed to its caller without guessing, and
-- walks the live list every vsync so the seats, widths and lifetime of
-- the intro instance are read off the running frame.
--
-- Also armed: FUN_800355F0, the "destroy every text actor" sweep that
-- ends the intro, so the banner's frame span is measured, not inferred.
--
-- Env vars:
--   LEGAIA_SSTATE      save state to load (else --scenario via run_probe.sh)
--   LEGAIA_FRAMES      vsyncs to capture after the load (default 1200)
--   LEGAIA_HOLD_BTN    pad button name to hold (e.g. RIGHT) - walks a field
--                      state into a random encounter; unset = no input
--   LEGAIA_HOLD_FRAMES vsyncs to hold it for (default 150)
--   LEGAIA_OUT_DIR     output directory (set by run_probe.sh)
--
-- Outputs (under LEGAIA_OUT_DIR):
--   w1d_spawns.csv   one row per FUN_8003541C / FUN_800355F0 call
--   w1d_list.csv     the live text-actor list, one row per node per change
--   w1d_intro_+N.rawsstate  save states 2 and 20 vsyncs after the intro
--                    spawn, so the banner's own display list can be walked
--                    offline with scripts/mednafen/widget-draw-sweep.py
--
-- Run (ambush - both the name labels and the formation line are up):
--   timeout 1700 bash scripts/pcsx-redux/run_probe.sh \
--       --lua scripts/pcsx-redux/autorun_w1d_intro_banner.lua \
--       --scenario rim_elm_queen_bee_battle --frames 3000 \
--       --isolate-config --out-dir captures/<run>
--
-- Run (ordinary round - walk into a random encounter):
--   LEGAIA_HOLD_BTN=RIGHT timeout 1500 bash scripts/pcsx-redux/run_probe.sh \
--       --lua scripts/pcsx-redux/autorun_w1d_intro_banner.lua \
--       --scenario karisto_sol_pre_encounter --frames 1500 \
--       --isolate-config --out-dir captures/<run>
--
-- Interpreter mode (the default) - exec breakpoints do not fire under
-- --fast / --timing. Keep --isolate-config: a retail battle load performs
-- an unaligned load, and the stock first-chance-exception mask pauses the
-- whole emulator on it.
--
-- PCSX-Redux probes do not exit on their own - always wrap in `timeout`.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1200)
local HOLD_BTN = probe.BTN[probe.getenv("LEGAIA_HOLD_BTN", "") or ""]
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD_FRAMES", 150)

local GAME_MODE_VA   = 0x8007B83C   -- _DAT_8007B83C game mode byte
local BATTLE_CTX_PTR = 0x8007BD24   -- pointer to the battle context
local TEXT_LIST_HEAD = 0x8007B460   -- gp[+0x148], text-actor list anchor
local SPAWN_FN       = 0x8003541C   -- FUN_8003541C(id, class, str, x, y, w, h, kind)
local CLEAR_FN       = 0x800355F0   -- FUN_800355F0() - destroy every text actor

local spawns, list_csv
local elapsed_now = 0
local last_sig = nil
local intro_frame = nil   -- vsync the composer's first spawn landed on
local states_left = { 2, 20 }

-- The composer's own return addresses: the name-label spawn at
-- 0x801DA218 and the formation-line spawn at 0x801DA314.
local COMPOSER_RA = { [0x801DA220] = true, [0x801DA31C] = true }

local function write_state(label)
    local ok = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(label .. ".rawsstate"), "CREATE")
        fh:writeMoveSlice(w)
        fh:close()
        PCSX.log("[w1d] state written: " .. label)
    end)
    if not ok then PCSX.log("[w1d] state write FAILED: " .. label) end
end

local function u32(v) return (tonumber(v) or 0) % 0x100000000 end

local function s16(v)
    v = (tonumber(v) or 0) % 0x10000
    if v >= 0x8000 then return v - 0x10000 end
    return v
end

local function ctx()
    local p = probe.read_u32(BATTLE_CTX_PTR)
    if p == nil then return nil end
    p = u32(p)
    if not probe.in_ram(p) then return nil end
    return p
end

-- Read a NUL-terminated string as printable ASCII with escapes, so a
-- name is identifiable in the CSV without dumping raw bytes anywhere
-- but the (gitignored) capture tree.
local function peek_str(addr, maxlen)
    if addr == nil or addr == 0 or not probe.in_ram(u32(addr)) then return "" end
    local out = {}
    for i = 0, (maxlen or 24) - 1 do
        local b = probe.read_u8(u32(addr) + i)
        if b == nil or b == 0 then break end
        if b >= 0x20 and b < 0x7F and b ~= 0x2C and b ~= 0x22 then
            out[#out + 1] = string.char(b)
        else
            out[#out + 1] = string.format("<%02X>", b)
        end
    end
    return table.concat(out)
end

local function walk_list()
    local anchor = probe.read_u32(TEXT_LIST_HEAD)
    if anchor == nil then return {} end
    anchor = u32(anchor)
    if not probe.in_ram(anchor) then return {} end
    local nodes, node, steps = {}, u32(probe.read_u32(anchor) or 0), 0
    while node ~= 0 and node ~= anchor and steps < 48 and probe.in_ram(node) do
        nodes[#nodes + 1] = {
            addr  = node,
            id    = probe.read_u16(node + 0x08) or 0,
            x     = s16(probe.read_u16(node + 0x0A)),
            y     = s16(probe.read_u16(node + 0x0C)),
            w     = s16(probe.read_u16(node + 0x0E)),
            h     = s16(probe.read_u16(node + 0x10)),
            str   = u32(probe.read_u32(node + 0x18) or 0),
            class = probe.read_u8(node + 0x1C) or 0,
            kind  = probe.read_u8(node + 0x1D) or 0,
        }
        node = u32(probe.read_u32(node) or 0)
        steps = steps + 1
    end
    return nodes
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = FRAMES,
    hold_button = HOLD_BTN,
    hold_frames = HOLD_BTN and HOLD_FRAMES or 0,

    on_arm = function()
        spawns = probe.csv_open(probe.out_path("w1d_spawns.csv"),
            "elapsed,event,ra,id,class,x,y,w,h,kind,strptr,mode,flow,timer,ambush,text")
        list_csv = probe.csv_open(probe.out_path("w1d_list.csv"),
            "elapsed,mode,flow,timer,n,idx,node,id,x,y,w,h,class,kind,strptr,text")

        probe.arm_breakpoint(SPAWN_FN, "Exec", 4, "text_actor_new", function()
            local r = PCSX.getRegisters()
            local sp = u32(r.GPR.n.sp)
            local c = ctx()
            if intro_frame == nil and COMPOSER_RA[u32(r.GPR.n.ra)] then
                intro_frame = elapsed_now
            end
            spawns:row("%d,spawn,0x%08X,%d,%d,%d,%d,%d,%d,%d,0x%08X,0x%02X,%s,%s,%s,%s",
                elapsed_now,
                u32(r.GPR.n.ra),
                u32(r.GPR.n.a0),
                u32(r.GPR.n.a1),
                s16(u32(r.GPR.n.a3)),
                s16(probe.read_u32(sp + 0x10) or 0),
                s16(probe.read_u32(sp + 0x14) or 0),
                s16(probe.read_u32(sp + 0x18) or 0),
                u32(probe.read_u32(sp + 0x1C) or 0),
                u32(r.GPR.n.a2),
                probe.read_u8(GAME_MODE_VA) or 0,
                c and string.format("0x%02X", probe.read_u8(c + 0x06) or 0) or "-",
                c and tostring(probe.read_u16(c + 0x6D6) or 0) or "-",
                c and tostring(probe.read_u8(c + 0x290) or 0) or "-",
                peek_str(u32(r.GPR.n.a2)))
        end)

        probe.arm_breakpoint(CLEAR_FN, "Exec", 4, "text_actor_clear_all", function()
            local r = PCSX.getRegisters()
            local c = ctx()
            spawns:row("%d,clear_all,0x%08X,,,,,,,,,0x%02X,%s,%s,%s,",
                elapsed_now,
                u32(r.GPR.n.ra),
                probe.read_u8(GAME_MODE_VA) or 0,
                c and string.format("0x%02X", probe.read_u8(c + 0x06) or 0) or "-",
                c and tostring(probe.read_u16(c + 0x6D6) or 0) or "-",
                c and tostring(probe.read_u8(c + 0x290) or 0) or "-")
        end)

        return {}
    end,

    on_capture = function(_, elapsed)
        elapsed_now = elapsed
        if intro_frame and #states_left > 0
            and elapsed >= intro_frame + states_left[1] then
            write_state(string.format("w1d_intro_+%d", states_left[1]))
            table.remove(states_left, 1)
        end
        local mode = probe.read_u8(GAME_MODE_VA) or 0
        local c = ctx()
        local flow = c and (probe.read_u8(c + 0x06) or 0) or -1
        local timer = c and (probe.read_u16(c + 0x6D6) or 0) or -1
        local nodes = walk_list()

        -- Log the list only when it changes, so the CSV stays a
        -- transition log rather than a per-frame dump.
        local parts = {}
        for i, n in ipairs(nodes) do
            parts[i] = string.format("%d/%d/%d/%d/%d/%d/%d",
                n.id, n.x, n.y, n.w, n.h, n.class, n.kind)
        end
        local sig = string.format("%02X|%d|%s", mode, flow, table.concat(parts, ";"))
        if sig ~= last_sig then
            last_sig = sig
            if #nodes == 0 then
                list_csv:row("%d,0x%02X,%d,%d,0,,,,,,,,,,,", elapsed, mode, flow, timer)
            end
            for i, n in ipairs(nodes) do
                list_csv:row("%d,0x%02X,%d,%d,%d,%d,0x%08X,%d,%d,%d,%d,%d,%d,%d,0x%08X,%s",
                    elapsed, mode, flow, timer, #nodes, i - 1, n.addr,
                    n.id, n.x, n.y, n.w, n.h, n.class, n.kind, n.str,
                    peek_str(n.str))
            end
        end
    end,

    on_summary = function()
        if spawns then spawns:close() end
        if list_csv then list_csv:close() end
        PCSX.log("[w1d] intro-banner capture done")
    end,
})
