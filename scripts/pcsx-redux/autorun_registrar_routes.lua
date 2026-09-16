-- autorun_registrar_routes.lua
--
-- Is the party model-pack registrar ever ENTERED over a stale buffer?
--
-- `FUN_8001E890` loads PROT 0874 (data\field\player.lzs), decompresses its
-- three sections and registers section 0's pack into the model pool. The
-- routine has one load-state word and one buffer, and the disassembly pins
-- both (ghidra/scripts/funcs/8001e890.txt):
--
--   gp+0x6AC = 0x8007B9C4   load state: 0 = never loaded (read the file,
--                           decompress, register), 1 = loaded (register
--                           only, over whatever the buffer holds), 2 = re-sum
--                           the raw file against gp+0x6B8 and decompress again
--   gp+0x6BC = 0x8007B9D4   the section-0 buffer FUN_8001E1B4 allocates once
--
--   0x8001EA44  lw v1,0x6ac(gp)      the gate
--   0x8001EA54  bne v1,2 -> 0x8001EAFC   state 1 skips the three decompress
--                                        calls and lands on the registrar
--   0x8001EAFC  lw a0,0x6bc(gp)      registrar entry (branch target)
--   0x8001EB0C  sw 1,0x6ac(gp)       state := 1
--   0x8001EB4C  jal tmd_register     one call per pack member, count = word 0,
--                                    member offset = word[1+i] * 4, unclamped
--
-- NB the sibling probe `autorun_model_pack_registrar.lua` labels 0x8007B83C
-- as "the == 2 gate". It is not: 0x8007B83C is gp+0x524, the game-mode
-- halfword. The gate is gp+0x6AC. This probe logs both.
--
-- The buffer is the same heap block in every save state, sane in field states
-- and battle-clobbered in battle states, so a registrar entry with state == 1
-- AFTER a battle load is the wild walk the 0874 thread is looking for. The
-- writers that would prevent it are all `sw zero` / `sw 2` into gp+0x6AC:
-- the SCUS mode transitions (0x800163B4, 0x800259D4, 0x80025D60, 0x800260A0),
-- the field overlay (0x801D15A8, 0x801E34C8) and the post-battle field
-- restore module PROT 0978 (0x801F6F04 writes 0, 0x801F723C writes 2). So the
-- rows this probe writes are, per route:
--
--   enter_fn   FUN_8001E890 entered: state word + buffer count BEFORE anything
--   gate       0x8001EA44: the state the bne reads
--   registrar  0x8001EAFC: state + count + first four member offsets
--   register   0x8001EB4C: the a0 each tmd_register call gets, and the index
--   state_w    a write to gp+0x6AC: pc, ra, the new value
--   pack_w     a write to gp+0x6BC: pc, ra, the new pointer
--
-- Input is a pad ladder on a clock that survives the title's blind-vsync
-- window (XA streaming stops GPU::Vsync delivery to Lua; the title tick
-- FUN_801DD35C and the field tick FUN_8001698C are exec breakpoints that keep
-- firing, and each advances the clock when no vsync did).
--
-- Outputs (probe.out_path, i.e. --out-dir):
--   registrar.csv  one row per event above
--   flow.csv       one row per game-mode / scene-name transition
--   manifest.txt
--
-- Env:
--   LEGAIA_SSTATE        state to resume (LEGAIA_NO_SSTATE=1 = cold boot)
--   LEGAIA_ROUTE         free-text label
--   LEGAIA_MASH          "BTN+BTN" pulsed every LEGAIA_MASH_EVERY clocks
--                        from LEGAIA_MASH_FROM to LEGAIA_MASH_UNTIL (0 = off)
--   LEGAIA_SEQ           "clock:BTN[+BTN],..." single 6-clock presses
--   LEGAIA_HOLD          "clock:BTN:clocks,..." held presses
--   LEGAIA_STOP_MODE     stop once game mode == this ... (-1 = off)
--   LEGAIA_STOP_SCENE    ... and/or the scene name == this ("" = any)
--   LEGAIA_STOP_ENTRIES  ... or after this many registrar entries (0 = off)
--   LEGAIA_SETTLE        clocks the stop condition must hold (default 120)
--   LEGAIA_MAX_CLOCK     hard cap (default 6000)
--   LEGAIA_BOOT_DELAY    vsyncs before the state load (default 60)
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")
local csv    = require("probe.csv")

local SSTATE     = env.getenv("LEGAIA_SSTATE", "")
local NO_SSTATE  = env.getenv("LEGAIA_NO_SSTATE", "") == "1"
local ROUTE      = env.getenv("LEGAIA_ROUTE", "route")
local MASH       = env.getenv("LEGAIA_MASH", "")
local MASH_EVERY = env.getenv_num("LEGAIA_MASH_EVERY", 20)
local MASH_FROM  = env.getenv_num("LEGAIA_MASH_FROM", 0)
local MASH_UNTIL = env.getenv_num("LEGAIA_MASH_UNTIL", 0)
local SEQ        = env.getenv("LEGAIA_SEQ", "")
local HOLD       = env.getenv("LEGAIA_HOLD", "")
local STOP_MODE  = env.getenv_num("LEGAIA_STOP_MODE", -1)
local STOP_SCENE = env.getenv("LEGAIA_STOP_SCENE", "")
local STOP_ENTRIES = env.getenv_num("LEGAIA_STOP_ENTRIES", 0)
local SETTLE     = env.getenv_num("LEGAIA_SETTLE", 120)
local MAX_CLOCK  = env.getenv_num("LEGAIA_MAX_CLOCK", 6000)
local BOOT_DELAY = env.getenv_num("LEGAIA_BOOT_DELAY", 60)

local FN_ENTRY   = 0x8001E890
local GATE       = 0x8001EA44
local REGISTRAR  = 0x8001EAFC
local REGISTER   = 0x8001EB4C
local STATE_W    = 0x8007B9C4   -- gp+0x6AC
local PACK_PTR   = 0x8007B9D4   -- gp+0x6BC
local MODE_HW    = 0x8007B83C   -- gp+0x524, game mode (low byte)
local SCENE_NAME = 0x8007050C
local BANK       = 0x8007B6F8   -- scene model-bank base (DAT_8007B6F8)
local TITLE_TICK = 0x801DD35C
local FIELD_TICK = 0x8001698C

local BTN = { UP = 4, RIGHT = 5, DOWN = 6, LEFT = 7, START = 3, SELECT = 0,
              TRIANGLE = 12, CIRCLE = 13, CROSS = 14, SQUARE = 15,
              L1 = 10, R1 = 11, L2 = 8, R2 = 9 }

local function parse_btns(s)
    local out = {}
    for tok in string.gmatch(s or "", "[^+%s]+") do
        if BTN[tok] then out[#out + 1] = BTN[tok] end
    end
    return out
end
local mash_btns = parse_btns(MASH)
local seq = {}
for tok in string.gmatch(SEQ, "[^,%s]+") do
    local t, b = string.match(tok, "(%d+):([%a+]+)")
    if t then seq[#seq + 1] = { at = tonumber(t), btns = parse_btns(b), name = b } end
end
local holds = {}
for tok in string.gmatch(HOLD, "[^,%s]+") do
    local t, b, n = string.match(tok, "(%d+):(%a+):(%d+)")
    if t and BTN[b] then holds[#holds + 1] = { at = tonumber(t), btn = BTN[b], len = tonumber(n), name = b } end
end

local function u8(a)  return mem.read_u8(a)  or 0 end
local function u16(a) return mem.read_u16(a) or 0 end
local function u32(a) return mem.read_u32(a) or 0 end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function regs()
    local r = PCSX.getRegisters()
    return r, (r.GPR and r.GPR.n) or {}
end
local function scene()
    local s = {}
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7f then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end
local function mode() return u16(MODE_HW) % 256 end

local reg_csv, flow_csv
local clock, vsync = 0, 0
local vsync_at_last_tick = -1
local n_entries, n_fn, n_register = 0, 0, 0
local last_flow = ""
local settled_since = nil
local done = false
local loaded = NO_SSTATE
local active = {}   -- button -> release clock

local function row(kind, extra)
    local p = u32(PACK_PTR)
    local cnt, o = -1, { -1, -1, -1, -1 }
    if p >= 0x80000000 and p < 0x80200000 then
        cnt = u32(p)
        for i = 1, 4 do o[i] = u32(p + i * 4) end
    end
    local _, n = regs()
    local r = PCSX.getRegisters()
    reg_csv:row("%d,%d,%s,0x%02X,%s,%d,0x%08X,%d,%d,%d,%d,%d,0x%08X,%d,0x%08X,0x%08X,0x%08X",
        clock, vsync, kind, mode(), scene(), u32(STATE_W), p, cnt,
        o[1], o[2], o[3], o[4], u32(BANK),
        extra.idx or -1, tou32(extra.a0 or 0), tou32(r.pc), tou32(n.ra))
end

-- The title / memory-card front end is one overlay-resident state machine
-- (FUN_801DD35C, sub-mode word 0x801F0204, two-row cursor 0x8007B820); both
-- are logged so a route through CONTINUE reads as the sub-modes it walked.
local TITLE_SUB   = 0x801F0204
local TITLE_CUR   = 0x8007B820
local function flow()
    local line = string.format("%02X/%s/%d/%d/%d", mode(), scene(), u32(STATE_W),
        u8(TITLE_SUB), u8(TITLE_CUR))
    if line ~= last_flow then
        last_flow = line
        flow_csv:row("%d,%d,0x%02X,%s,%d,0x%08X,%d,%d,%d", clock, vsync, mode(), scene(),
            u32(STATE_W), u32(PACK_PTR), n_entries, u8(TITLE_SUB), u8(TITLE_CUR))
    end
end

local function stop_ok()
    if STOP_ENTRIES > 0 and n_entries >= STOP_ENTRIES then return true end
    if STOP_MODE < 0 and STOP_SCENE == "" then return false end
    if STOP_MODE >= 0 and mode() ~= STOP_MODE then return false end
    if STOP_SCENE ~= "" and scene() ~= STOP_SCENE then return false end
    return true
end

local function pad_step()
    for b, until_c in pairs(active) do
        if clock >= until_c then pad.release(b); active[b] = nil end
    end
    if #mash_btns > 0 and clock >= MASH_FROM and (MASH_UNTIL == 0 or clock < MASH_UNTIL)
        and clock % MASH_EVERY == 0 then
        for _, b in ipairs(mash_btns) do pad.force(b); active[b] = clock + 5 end
    end
    for _, s in ipairs(seq) do
        if s.at == clock then
            for _, b in ipairs(s.btns) do pad.force(b); active[b] = clock + 6 end
            PCSX.log(string.format("[routes] c%d press %s", clock, s.name))
        end
    end
    for _, h in ipairs(holds) do
        if h.at == clock then
            pad.force(h.btn); active[h.btn] = clock + h.len
            PCSX.log(string.format("[routes] c%d hold %s for %d", clock, h.name, h.len))
        end
    end
end

local function finish(reason)
    if done then return end
    done = true
    for b, _ in pairs(active) do pad.release(b) end
    PCSX.log(string.format("[routes] %s: %s clock=%d vsync=%d fn_entries=%d registrar_entries=%d register_calls=%d",
        ROUTE, reason, clock, vsync, n_fn, n_entries, n_register))
    if reg_csv then reg_csv:close() end
    if flow_csv then flow_csv:close() end
    PCSX.quit(0)
end

local function tick()
    if done or not loaded then return end
    clock = clock + 1
    flow()
    pad_step()
    if stop_ok() then
        if settled_since == nil then settled_since = clock end
        if clock - settled_since >= SETTLE then finish("stop condition settled") end
    else
        settled_since = nil
    end
    if clock >= MAX_CLOCK then finish("max clock") end
end

local function arm()
    PCSX.log(string.format("== registrar routes == route=%s sstate=%s", ROUTE, NO_SSTATE and "(cold boot)" or SSTATE))
    env.write_manifest("autorun_registrar_routes.lua", {
        route = ROUTE, sstate = NO_SSTATE and "(cold boot)" or SSTATE,
        mash = MASH, mash_every = MASH_EVERY, mash_from = MASH_FROM, mash_until = MASH_UNTIL,
        seq = SEQ, hold = HOLD, stop_mode = STOP_MODE, stop_scene = STOP_SCENE,
        stop_entries = STOP_ENTRIES, settle = SETTLE, max_clock = MAX_CLOCK,
    })
    reg_csv = csv.open(env.out_path("registrar.csv"),
        "clock,vsync,kind,mode,scene,state,pack,count,off0,off1,off2,off3,bank,idx,a0,pc,ra")
    flow_csv = csv.open(env.out_path("flow.csv"), "clock,vsync,mode,scene,state,pack,entries,title_sub,title_cursor")

    bp.arm(FN_ENTRY, "Exec", 4, "fn_entry", function()
        n_fn = n_fn + 1
        row("enter_fn", {})
    end)
    bp.arm(GATE, "Exec", 4, "gate", function() row("gate", {}) end)
    bp.arm(REGISTRAR, "Exec", 4, "registrar", function()
        n_entries = n_entries + 1
        row("registrar", {})
    end)
    local idx = 0
    bp.arm(REGISTER, "Exec", 4, "register", function()
        local _, n = regs()
        n_register = n_register + 1
        idx = tonumber(n.s0) or 0
        row("register", { idx = idx, a0 = n.a0 })
    end)
    bp.arm(STATE_W, "Write", 4, "state_w", function()
        -- the new value is not yet visible at the callback; log the register
        -- file instead: every writer is `sw zero` or `sw v1/v0`, so v0/v1
        -- carry it, and the pc names which
        local _, n = regs()
        row("state_w", { a0 = n.v0, idx = tonumber(n.v1) or -1 })
    end)
    bp.arm(PACK_PTR, "Write", 4, "pack_w", function()
        local _, n = regs()
        row("pack_w", { a0 = n.v0 })
    end)
    -- the clocks
    bp.arm(TITLE_TICK, "Exec", 4, "title_tick", function()
        if vsync == vsync_at_last_tick then tick() end
        vsync_at_last_tick = vsync
    end)
    bp.arm(FIELD_TICK, "Exec", 4, "field_tick", function()
        if vsync == vsync_at_last_tick then tick() end
        vsync_at_last_tick = vsync
    end)
end

local armed = false
local function on_vsync()
    vsync = vsync + 1
    if not armed then
        armed = true
        local ok, err = pcall(arm)
        if not ok then PCSX.log("[routes] arm failed: " .. tostring(err)); PCSX.quit(3); return end
    end
    if not loaded and vsync >= BOOT_DELAY then
        if sstate.load(SSTATE) then
            loaded = true
            PCSX.log("[routes] resumed " .. SSTATE)
        else
            PCSX.log("[routes] FAILED to load " .. SSTATE)
            PCSX.quit(2)
            return
        end
    end
    tick()
end

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
