-- autorun_w7c_steal_oracle.lua
--
-- Retail player-steal oracle. The steal is not a battle command: it is a
-- side branch of the SCUS anim commit `FUN_8004AD80`, taken on the frame an
-- ENEMY seat's death animation (anim-table byte 4, HP `+0x14C` == 0) is
-- committed. Disassembly of the branch (`0x8004B29C..0x8004B660`):
--
--   0x8004B2AC  lw  *(0x801C8FE0 + (seat-3)*4)  -- item this enemy stole
--               from the party; non-zero -> "Recovered / Took the stolen"
--               path, never a steal
--   0x8004B3C0  ctx(gp+0xA0C)+0x13 < 3          -- acting seat is a party seat
--   0x8004B3D4  ctx+0x27 == 0, then := 1        -- one attempt per latch
--   0x8004B400  mult = 1, or 2 when any living party seat's record word
--               +0xF8 has bit 0x20000
--   0x8004B488  *(0x8007BAC0) == 0
--   0x8004B4BC  actor(seat ctx+0x13)+0x1DE == 3
--   0x8004B500  record +0xF4 bit 0x10000 (the acting character's steal
--               passive)
--   0x8004B514  rand() (BIOS A0:2F); 0x8004B580 sltu (rand%100) < chance*mult
--               with chance = u8 0x80077828[monster_id*2], monster id =
--               u8 0x8007BD09[seat]; or the forced pair
--               *(0x8007B98C) != 0 && *(i16 0x8007BA58) != 0
--   0x8004B5BC  FUN_80042F4C(item) == 99 -> no steal (bag cannot take it)
--   0x8004B5EC  caption "Stole the ..." (FUN_8003CA78/FUN_8003CB54/
--               FUN_8003CAC4), window 0x5B via 0x801D8DE8, ctx+0x18 := 0x5B
--   0x8004B654  FUN_800421D4(item, 1) -- the bag grant
--
-- This probe loads a battle state, optionally writes the synthetic gates
-- (say so wherever a result rests on them), presses Begin and logs every
-- step of the branch plus every reader of the steal table and every
-- writer of the bag.
--
-- Env:
--   LEGAIA_STEAL_BIT  1 = OR 0x10000 into record +0xF4 of the character in
--                     party seat LEGAIA_STEAL_SEAT (default 0) - synthetic:
--                     stands in for an equipped steal accessory
--   LEGAIA_STEAL_ALL  1 = apply the bit pokes to every party seat 0..2
--   LEGAIA_DOUBLE_BIT 1 = OR 0x20000 into that record's +0xF8 (synthetic)
--   LEGAIA_ENEMY_HP   N = write u16 N into every enemy seat's +0x14C once
--   LEGAIA_FORCE_MOD  N = overwrite v0 (= rand()%100) at 0x8004B580 with N
--   LEGAIA_PRESS      "<vsync>:<button>:<for>,..." after the state loads
--   LEGAIA_MASH       "<button>:<period>:<for>" from LEGAIA_MASH_FROM
--   LEGAIA_MAX_TICKS  stop after this many vsyncs
--
-- Output: w7c_steal.csv (one row per branch event), w7c_steal.log.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local pad   = require("probe.pad")
local bit   = require("bit")

local ACTORS     = 0x801C9370
local MON_ID     = 0x8007BD09   -- + seat (seats 3.. are enemies)
local CHAR_ID    = 0x8007BD10   -- + seat (party seats 0..2)
local REC_BASE   = 0x80084708
local REC_STRIDE = 0x414
local STEAL_TAB  = 0x80077828
local BAG        = 0x80085958
local GAME_MODE  = 0x8007B83C

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local MAX_TICKS  = probe.getenv_num("LEGAIA_MAX_TICKS", 1500)
local STEAL_BIT  = probe.getenv("LEGAIA_STEAL_BIT", "") == "1"
local DOUBLE_BIT = probe.getenv("LEGAIA_DOUBLE_BIT", "") == "1"
local STEAL_SEAT = probe.getenv_num("LEGAIA_STEAL_SEAT", 0)
local STEAL_ALL  = probe.getenv("LEGAIA_STEAL_ALL", "") == "1"
local ENEMY_HP   = probe.getenv_num("LEGAIA_ENEMY_HP", -1)
local FORCE_MOD  = probe.getenv_num("LEGAIA_FORCE_MOD", -1)
local MASH_FROM  = probe.getenv_num("LEGAIA_MASH_FROM", 0)
local OUT_DIR    = probe.getenv("LEGAIA_OUT_DIR", "captures/w7c_steal_oracle")

local BTN = {
    up = pad.BTN.UP, down = pad.BTN.DOWN, left = pad.BTN.LEFT,
    right = pad.BTN.RIGHT, cross = pad.BTN.CROSS, circle = pad.BTN.CIRCLE,
    triangle = pad.BTN.TRIANGLE, square = pad.BTN.SQUARE,
    start = pad.BTN.START, select = pad.BTN.SELECT,
}
local presses = {}
for tok in string.gmatch(probe.getenv("LEGAIA_PRESS", ""), "[^,%s]+") do
    local at, name, dur = string.match(tok, "^(%d+):(%a+):(%d+)$")
    presses[#presses + 1] = { at = tonumber(at), dur = tonumber(dur), btn = BTN[string.lower(name)], name = name }
end
local mash = nil
do
    local name, period, dur = string.match(probe.getenv("LEGAIA_MASH", ""), "^(%a+):(%d+):(%d+)$")
    if name then mash = { btn = BTN[string.lower(name)], period = tonumber(period), dur = tonumber(dur) } end
end

os.execute(string.format("mkdir -p %q", OUT_DIR))
local CSV = probe.csv_open(probe.out_path("w7c_steal.csv"),
    "vsync,event,pc,ra,seat,monster,chance,item,mult,rand_raw,rand_mod,threshold,ret,detail")
local LOGF = io.open(probe.out_path("w7c_steal.log"), "w")
local function log(s)
    PCSX.log("[w7c_steal] " .. s)
    if LOGF then LOGF:write(s .. "\n"); LOGF:flush() end
end
local function u8(a) return mem.read_u8(a) or 0 end
local function u16(a) return (mem.read_u16(a) or 0) % 0x10000 end
local function u32n(v) v = tonumber(v) or 0; if v < 0 then v = v + 4294967296 end; return v end
local function hex32(v) return string.format("0x%08X", u32n(v)) end
local function rd32(a) return u32n(mem.read_u32(a) or 0) end

local vsync, loaded_at, armed, done, poked = 0, nil, false, false, false
local bag_dirty = {}
local cur = { seat = -1, monster = -1, chance = -1, item = -1, mult = -1, raw = -1, mod = -1, thr = -1 }

local function row(event, pc, ra, ret, detail)
    CSV:row("%d,%s,%s,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s", vsync, event, hex32(pc), hex32(ra),
        cur.seat, cur.monster, cur.chance, cur.item, cur.mult, cur.raw, cur.mod, cur.thr, ret, detail or "")
    CSV.fh:flush()
end

local function ctx_ptr()
    local r = PCSX.getRegisters()
    return rd32(u32n(r.GPR.n.gp) + 0xA0C)
end

local function party_line()
    local out = {}
    for s = 0, 6 do
        local a = rd32(ACTORS + s * 4)
        if a >= 0x80000000 and a < 0x80200000 then
            local id = s < 3 and u8(CHAR_ID + s) or u8(MON_ID + s)
            out[#out + 1] = string.format("seat%d:%s id=%d hp=%d 1de=%d", s, hex32(a), id, u16(a + 0x14C), u8(a + 0x1DE))
        end
    end
    return table.concat(out, " | ")
end

local function do_pokes()
    log("pre-poke: " .. party_line())
    -- Enemy ids occupy 0x8007BD0C..0x8007BD0F (seats 3..6); 0x8007BD10 is
    -- already party seat 0's character id.
    for s = 3, 6 do
        local a = rd32(ACTORS + s * 4)
        local id = u8(MON_ID + s)
        if a >= 0x80000000 and a < 0x80200000 and id ~= 0 then
            log(string.format("enemy seat %d monster %d steal row [chance %d, item 0x%02X]", s, id,
                u8(STEAL_TAB + id * 2), u8(STEAL_TAB + id * 2 + 1)))
            if ENEMY_HP >= 0 then
                mem.write_u16(a + 0x14C, ENEMY_HP)
                log(string.format("POKE (synthetic) enemy seat %d HP := %d", s, ENEMY_HP))
            end
        end
    end
    local seats = { STEAL_SEAT }
    if STEAL_ALL then seats = { 0, 1, 2 } end
    for _, seat in ipairs(seats) do
        local cid = u8(CHAR_ID + seat)
        if cid >= 1 and cid <= 8 then
            local rec = REC_BASE + (cid - 1) * REC_STRIDE
            local f4, f8 = rd32(rec + 0xF4), rd32(rec + 0xF8)
            log(string.format("seat %d char %d record %s +0xF4=%s +0xF8=%s", seat, cid, hex32(rec), hex32(f4), hex32(f8)))
            if STEAL_BIT then
                mem.write_u32(rec + 0xF4, bit.bor(f4, 0x10000))
                log(string.format("POKE (synthetic) seat %d +0xF4 |= 0x10000", seat))
            end
            if DOUBLE_BIT then
                mem.write_u32(rec + 0xF8, bit.bor(f8, 0x20000))
                log(string.format("POKE (synthetic) seat %d +0xF8 |= 0x20000", seat))
            end
        end
    end
    local bagn = 0
    for i = 0, 255 do if u8(BAG + i * 2) ~= 0 then bagn = bagn + 1 end end
    log(string.format("bag occupied slots %d / 256; ctx27 via gp unknown until a bp", bagn))
end

local function arm_all()
    -- Branch entry: the stolen-by-enemy word for this seat.
    bp.arm(0x8004B2AC, "Exec", 4, "stolen_word", function()
        local r = PCSX.getRegisters()
        local s3 = u32n(r.GPR.n.s3)
        cur = { seat = s3, monster = u8(MON_ID + s3), chance = -1, item = -1, mult = -1, raw = -1, mod = -1, thr = -1 }
        local held = rd32(u32n(r.GPR.n.s2))
        local c = ctx_ptr()
        row("death_commit", 0x8004B2AC, r.GPR.n.ra, held, string.format("ctx=%s ctx13=%d ctx27=%d bac0=%s actor=%s",
            hex32(c), u8(c + 0x13), u8(c + 0x27), hex32(rd32(0x8007BAC0)), hex32(rd32(ACTORS + u8(c + 0x13) * 4))))
        log(string.format("f=%d death commit seat %d monster %d held_stolen=%d ctx13=%d ctx27=%d | %s",
            vsync, s3, cur.monster, held, u8(c + 0x13), u8(c + 0x27), party_line()))
    end)
    bp.arm(0x8004B3E4, "Exec", 4, "latch27", function()
        local c = ctx_ptr()
        row("latch_ctx27", 0x8004B3E4, 0, u8(c + 0x27), "")
    end)
    bp.arm(0x8004B488, "Exec", 4, "mult", function()
        local r = PCSX.getRegisters()
        cur.mult = u32n(r.GPR.n.s0)
        row("mult", 0x8004B488, 0, cur.mult, string.format("bac0=%s", hex32(rd32(0x8007BAC0))))
    end)
    bp.arm(0x8004B4C4, "Exec", 4, "gate_1de", function()
        local r = PCSX.getRegisters()
        row("gate_1de", 0x8004B4C4, 0, u32n(r.GPR.n.v1), "actor+0x1DE (needs 3)")
    end)
    bp.arm(0x8004B50C, "Exec", 4, "gate_f4", function()
        local r = PCSX.getRegisters()
        row("gate_f4", 0x8004B50C, 0, u32n(r.GPR.n.v0), "record+0xF4 & 0x10000")
    end)
    bp.arm(0x8004B51C, "Exec", 4, "rand_ret", function()
        local r = PCSX.getRegisters()
        cur.raw = u32n(r.GPR.n.v0)
        cur.chance = u8(STEAL_TAB + cur.monster * 2)
        cur.item = u8(STEAL_TAB + cur.monster * 2 + 1)
        row("rand", 0x8004B51C, 0, cur.raw, "")
    end)
    bp.arm(0x8004B580, "Exec", 4, "roll_cmp", function()
        local r = PCSX.getRegisters()
        cur.mod = u32n(r.GPR.n.v0)
        cur.thr = u32n(r.GPR.n.t2)
        local detail = ""
        if FORCE_MOD >= 0 then
            detail = string.format("FORCED (synthetic) rand mod 100 %d -> %d", cur.mod, FORCE_MOD)
            r.GPR.n.v0 = FORCE_MOD
            cur.mod = FORCE_MOD
        end
        row("roll", 0x8004B580, 0, (cur.mod < cur.thr) and 1 or 0, detail)
        log(string.format("f=%d ROLL monster %d chance %d mult %d raw %d mod %d thr %d -> %s %s",
            vsync, cur.monster, cur.chance, cur.mult, cur.raw, cur.mod, cur.thr,
            (cur.mod < cur.thr) and "PASS" or "fail", detail))
    end)
    bp.arm(0x8004B58C, "Exec", 4, "roll_fail_cheat", function()
        row("roll_fail_path", 0x8004B58C, 0, rd32(0x8007B98C), string.format("ba58=%d", u16(0x8007BA58)))
    end)
    bp.arm(0x8004B5C4, "Exec", 4, "bag_probe_ret", function()
        local r = PCSX.getRegisters()
        row("bag_probe", 0x8004B5C4, 0, u32n(r.GPR.n.v0) % 0x10000, "FUN_80042F4C(item) (99 = refuse)")
    end)
    bp.arm(0x8004B628, "Exec", 4, "caption", function()
        local r = PCSX.getRegisters()
        row("caption", 0x8004B628, 0, u32n(r.GPR.n.a0), string.format("text@0x80077A08 msgptr=%s", hex32(rd32(0x800774AC))))
    end)
    -- Arguments are read at the CALLEE entry: at the `jal` itself a0 still
    -- sits in the `lbu`'s load-delay slot and reads stale.
    bp.arm(0x80042F4C, "Exec", 4, "bag_probe_entry", function()
        local r = PCSX.getRegisters()
        if u32n(r.GPR.n.ra) ~= 0x8004B5C4 then return end
        row("bag_probe_arg", 0x80042F4C, r.GPR.n.ra, u32n(r.GPR.n.a0), "item id passed to FUN_80042F4C")
    end)
    bp.arm(0x800421D4, "Exec", 4, "grant", function()
        local r = PCSX.getRegisters()
        if u32n(r.GPR.n.ra) ~= 0x8004B65C then return end
        row("grant", 0x800421D4, r.GPR.n.ra, u32n(r.GPR.n.a0), string.format("count=%d", u32n(r.GPR.n.a1)))
        log(string.format("f=%d GRANT item 0x%02X x%d", vsync, u32n(r.GPR.n.a0), u32n(r.GPR.n.a1)))
    end)
    bp.arm(0x8004B660, "Exec", 4, "branch_exit", function()
        row("branch_exit", 0x8004B660, 0, 0, "")
    end)
    -- Every reader of the steal table and every writer of the bag.
    bp.arm(STEAL_TAB, "Read", 0x1E0, "steal_tab_read", function(addr)
        local r = PCSX.getRegisters()
        row("tab_read", u32n(r.pc), r.GPR.n.ra, 0, string.format("addr=%s", hex32(addr or 0)))
    end)
    bp.arm(BAG, "Write", 0x200, "bag_write", function(addr)
        local r = PCSX.getRegisters()
        local a = u32n(addr or 0)
        row("bag_write", u32n(r.pc), r.GPR.n.ra, 0, string.format("addr=%s slot=%d byte=%d", hex32(a),
            math.floor((a - BAG) / 2), (a - BAG) % 2))
        bag_dirty[#bag_dirty + 1] = a
    end)
    -- Who clears / sets the one-attempt latch ctx+0x27.
    local c = rd32(u32n(PCSX.getRegisters().GPR.n.gp) + 0xA0C)
    if c >= 0x80000000 and c < 0x80200000 then
        bp.arm(c + 0x27, "Write", 1, "ctx27_write", function()
            local r = PCSX.getRegisters()
            row("ctx27_write", u32n(r.pc), r.GPR.n.ra, u8(c + 0x27), string.format("ctx=%s", hex32(c)))
            log(string.format("f=%d ctx+0x27 write pc=%s ra=%s (old %d)", vsync, hex32(r.pc), hex32(r.GPR.n.ra), u8(c + 0x27)))
        end)
        log("ctx = " .. hex32(c))
    end
    armed = true
    log("armed")
end

local function finish(why)
    if done then return end
    done = true
    log(string.format("%s at tick %d mode=0x%02X", why, vsync, u8(GAME_MODE)))
    local bagn = 0
    for i = 0, 255 do if u8(BAG + i * 2) ~= 0 then bagn = bagn + 1 end end
    log(string.format("bag occupied slots at end %d / 256", bagn))
    pcall(function() bp.disarm() end)
    CSV:close()
    if LOGF then LOGF:close() end
    PCSX.quit(0)
end

local function on_vsync()
    if done then return end
    vsync = vsync + 1
    if loaded_at == nil then
        if vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then log("FATAL load " .. SSTATE); finish("load failed"); return end
            loaded_at = vsync
            log(string.format("state loaded at %d mode=0x%02X", vsync, u8(GAME_MODE)))
        end
        return
    end
    if not armed then arm_all(); return end
    if not poked then poked = true; do_pokes() end
    if #bag_dirty > 0 then
        for _, a in ipairs(bag_dirty) do
            local sl = math.floor((a - BAG) / 2)
            log(string.format("f=%d bag slot %d now [id 0x%02X, count %d]", vsync, sl, u8(BAG + sl * 2), u8(BAG + sl * 2 + 1)))
        end
        bag_dirty = {}
    end
    local t = vsync - loaded_at
    for _, p in ipairs(presses) do
        if t == p.at then pad.force(p.btn); log("press " .. p.name .. " @" .. t)
        elseif t == p.at + p.dur then pad.release(p.btn) end
    end
    if mash and t >= MASH_FROM then
        local ph = (t - MASH_FROM) % mash.period
        if ph == 0 then pad.force(mash.btn) elseif ph == mash.dur then pad.release(mash.btn) end
    end
    if vsync >= MAX_TICKS then finish("max ticks") end
end

log("=== autorun_w7c_steal_oracle ===")
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
