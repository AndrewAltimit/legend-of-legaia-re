-- autorun_w5a_flag_watch.lua
--
-- Single-system-flag write watch across a scene entry.
--
-- Answers "does retail's own entry into <scene> write story flag F, and
-- which script arm does it" for one flag at a time, by arming three
-- observers before the entry runs and letting the scene load happen
-- under them:
--
--   1. raw Write-watch on the flag's BANK BYTE (`0x80085758 + (F >> 3)`,
--      width 1) - catches every writer of the byte, including the save
--      block's own bulk restore, and names pc + ra.
--   2. Exec-bp on the bank SET helper `FUN_8003CE08`, filtered a0 == F.
--   3. Exec-bp on the bank CLEAR helper `FUN_8003CE34`, filtered a0 == F.
--
-- The exec-bps name the calling arm directly (ra); the byte watch is the
-- fail-safe for a writer that bypasses the helpers (the SC-block restore
-- does exactly that). Both are armed from the seeded state's FIRST tick,
-- so an entry script that runs before the field mode word settles is
-- still covered - which is the whole point: the flag question a
-- mid-scene capture cannot answer is what the ENTRY did.
--
-- Launch (MUST be -interpreter -debugger; Lua BPs are dead under --fast):
--   LEGAIA_SSTATE=<pre-entry state> LEGAIA_WATCH_FLAG=1758 \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_w5a_flag_watch.lua --frames 600
--
-- Env:
--   LEGAIA_WATCH_FLAG   flag index, decimal (default 1758 = 0x6DE)
--   LEGAIA_BOOT_DELAY   vsyncs before the state load (default 60)
--   LEGAIA_FRAMES       vsyncs to keep running AFTER field mode settles
--   LEGAIA_MAX_TICKS    hard stop (default 4000)
--   LEGAIA_TRACE_EVERY  per-vsync trace row cadence (default 1)
--   LEGAIA_POKE_XZ      "x,z" retail world units; once in field mode, force
--                       the player actor there every vsync (causality test
--                       for a flag suspected of being a position latch)
--   LEGAIA_POKE_AFTER   field vsyncs to wait before poking (default 120)
--
-- Output (probe out dir):
--   w5a_flag_hits.csv    tick,label,addr,pc,ra,a0,byte_before,byte_after,
--                        bit,mode,scene,px,pz,s8,cursor
--   w5a_flag_trace.csv   tick,mode,scene,bit,byte,px,pz
--   w5a_flag.log         human log
--
-- A write is "caught" when a hits row has a non-zero ra. The `bit` column
-- is the watched flag's own bit AFTER the write, so a clear-then-set pair
-- reads 1 -> 0 -> 1 in order.
--
-- `s8` is the field VM's own bytecode cursor at the call (the register the
-- dispatcher advances with `addiu s8, s8, N`) and `cursor` is the four
-- bytes at `s8`, so a hit names the RECORD that issued the write: two hits
-- whose `s8` differ by a record-start delta are two records, and two hits
-- at one `s8` are one record run twice.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local bit   = require("bit")

local GAME_MODE      = 0x8007B83C
local SCENE_NAME     = 0x8007050C
local PLAYER_PTR     = 0x8007C364
local FLAG_BANK_BASE = 0x80085758
local FLAG_SET_PC    = 0x8003CE08
local FLAG_CLR_PC    = 0x8003CE34

local FLAG        = probe.getenv_num("LEGAIA_WATCH_FLAG", 1758)
local SSTATE      = probe.getenv("LEGAIA_SSTATE", "")
local BOOT_DELAY  = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local POST_FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local MAX_TICKS   = probe.getenv_num("LEGAIA_MAX_TICKS", 4000)
local TRACE_EVERY = probe.getenv_num("LEGAIA_TRACE_EVERY", 1)
-- Causality knob: once the field mode word has been settled for
-- LEGAIA_POKE_AFTER vsyncs, write LEGAIA_POKE_XZ ("x,z", retail world
-- units) into the player actor's `+0x14`/`+0x18` every vsync. A flag whose
-- value is a player-position test then flips under the poke and a flag that
-- is a progress latch does not - which is the difference a position-only
-- reading cannot otherwise prove.
local POKE_XZ    = probe.getenv("LEGAIA_POKE_XZ", "")
local POKE_AFTER = probe.getenv_num("LEGAIA_POKE_AFTER", 120)
local poke_x, poke_z = nil, nil
if POKE_XZ ~= "" then
    local a, b = string.match(POKE_XZ, "(-?%d+),(-?%d+)")
    if a then poke_x, poke_z = tonumber(a), tonumber(b) end
end

local BANK_BYTE = FLAG_BANK_BASE + math.floor(FLAG / 8)
local BANK_MASK = bit.rshift(0x80, FLAG % 8)

local HITS = probe.csv_open(probe.out_path("w5a_flag_hits.csv"),
    "tick,label,addr,pc,ra,a0,byte_before,byte_after,bit,mode,scene,px,pz,s8,cursor")
local TRACE = probe.csv_open(probe.out_path("w5a_flag_trace.csv"),
    "tick,mode,scene,bit,byte,px,pz")
local LOGF = io.open(probe.out_path("w5a_flag.log"), "w")

local function log(s)
    PCSX.log("[w5a] " .. s)
    if LOGF then LOGF:write(s .. "\n"); LOGF:flush() end
end

local function u8(a) return mem.read_u8(a) or 0 end
local function u32(a) return mem.read_u32(a) or 0 end
-- LuaJIT's bit ops return SIGNED 32-bit, so a kernel-segment address comes
-- back negative and both `%X` and a `== 0x80000000` test misread it. Fold
-- back into the unsigned range before formatting or comparing.
local function u32n(v) v = tonumber(v) or 0; if v < 0 then v = v + 4294967296 end; return v end
local function hex32(v) return string.format("0x%08X", u32n(v)) end

local function scene_name()
    local s = {}
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7F then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

local function player_ptr()
    local p = u32n(u32(PLAYER_PTR))
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end

local function player_xz()
    local p = player_ptr()
    if p == nil then return 0, 0 end
    local x = mem.read_u16(p + 0x14) or 0
    local z = mem.read_u16(p + 0x18) or 0
    x = x % 0x10000
    z = z % 0x10000
    if x >= 0x8000 then x = x - 0x10000 end
    if z >= 0x8000 then z = z - 0x10000 end
    return x, z
end

local vsync       = 0
local loaded_at   = nil
local armed       = false
local field_ticks = 0
local done        = false
local prev_byte   = -1
local hits        = 0

local function cursor_bytes(s8)
    if s8 < 0x80000000 or s8 >= 0x80200000 then return "-" end
    local out = {}
    for i = 0, 3 do out[#out + 1] = string.format("%02X", u8(s8 + i)) end
    return table.concat(out)
end

local function record(label, addr, pc, ra, a0, before, s8)
    local after = u8(BANK_BYTE)
    local b = (bit.band(after, BANK_MASK) ~= 0) and 1 or 0
    local px, pz = player_xz()
    s8 = u32n(s8 or 0)
    HITS:row("%d,%s,%s,%s,%s,%d,0x%02X,0x%02X,%d,0x%02X,%s,%d,%d,%s,%s",
        vsync, label, hex32(addr), hex32(pc), hex32(ra), a0, before, after, b,
        u8(GAME_MODE), scene_name(), px, pz, hex32(s8), cursor_bytes(s8))
    HITS.fh:flush()
    hits = hits + 1
    log(string.format(
        "HIT %-14s tick=%d pc=%s ra=%s a0=%d byte 0x%02X->0x%02X bit=%d mode=0x%02X scene=%s p=(%d,%d)",
        label, vsync, hex32(pc), hex32(ra), a0, before, after, b,
        u8(GAME_MODE), scene_name(), px, pz) ..
        string.format(" s8=%s[%s]", hex32(s8), cursor_bytes(s8)))
end

local function regs() return PCSX.getRegisters() end

local function arm_all()
    bp.arm(BANK_BYTE, "Write", 1, "flag_byte", function()
        local r = regs()
        record("byte_write", BANK_BYTE,
            bit.band(tonumber(r.pc), 0xFFFFFFFF),
            bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF),
            bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFF),
            prev_byte >= 0 and prev_byte or u8(BANK_BYTE),
            tonumber(r.GPR.n.s8))
        prev_byte = u8(BANK_BYTE)
    end)
    bp.arm(FLAG_SET_PC, "Exec", 4, "flag_set", function()
        local r = regs()
        local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFF)
        if a0 ~= FLAG then return end
        record("helper_set", BANK_BYTE, FLAG_SET_PC,
            bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF), a0, u8(BANK_BYTE),
            tonumber(r.GPR.n.s8))
    end)
    bp.arm(FLAG_CLR_PC, "Exec", 4, "flag_clear", function()
        local r = regs()
        local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFF)
        if a0 ~= FLAG then return end
        record("helper_clear", BANK_BYTE, FLAG_CLR_PC,
            bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF), a0, u8(BANK_BYTE),
            tonumber(r.GPR.n.s8))
    end)
    armed = true
    log(string.format("armed at tick %d: flag %d (0x%X) -> byte 0x%08X mask 0x%02X",
        vsync, FLAG, FLAG, BANK_BYTE, BANK_MASK))
end

local function finish(why)
    if done then return end
    done = true
    log(string.format("%s at tick %d; hits=%d final byte=0x%02X bit=%d scene=%s mode=0x%02X",
        why, vsync, hits, u8(BANK_BYTE),
        (bit.band(u8(BANK_BYTE), BANK_MASK) ~= 0) and 1 or 0,
        scene_name(), u8(GAME_MODE)))
    pcall(function() bp.disarm() end)
    HITS:close(); TRACE:close()
    if LOGF then LOGF:close() end
    PCSX.quit(0)
end

local function on_vsync()
    if done then return end
    vsync = vsync + 1

    if loaded_at == nil then
        if SSTATE == "" then
            loaded_at = vsync
            log("no LEGAIA_SSTATE: watching from the current session")
        elseif vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then
                log("FATAL: could not load " .. SSTATE)
                finish("load failed")
                return
            end
            loaded_at = vsync
            log(string.format("state loaded at tick %d; mode=0x%02X scene=%s",
                vsync, u8(GAME_MODE), scene_name()))
        end
        return
    end

    if not armed then
        prev_byte = u8(BANK_BYTE)
        arm_all()
        return
    end

    if (vsync % TRACE_EVERY) == 0 then
        local byte = u8(BANK_BYTE)
        local px, pz = player_xz()
        TRACE:row("%d,0x%02X,%s,%d,0x%02X,%d,%d", vsync, u8(GAME_MODE), scene_name(),
            (bit.band(byte, BANK_MASK) ~= 0) and 1 or 0, byte, px, pz)
    end
    prev_byte = u8(BANK_BYTE)

    if u8(GAME_MODE) == 0x03 then
        field_ticks = field_ticks + 1
        if poke_x ~= nil and field_ticks >= POKE_AFTER then
            local p = player_ptr()
            if p ~= nil then
                mem.write_u16(p + 0x14, poke_x % 0x10000)
                mem.write_u16(p + 0x18, poke_z % 0x10000)
                if field_ticks == POKE_AFTER then
                    log(string.format("poking player to (%d,%d) from field tick %d",
                        poke_x, poke_z, field_ticks))
                end
            end
        end
        if field_ticks >= POST_FRAMES then finish("post-field window done") end
    end
    if vsync >= MAX_TICKS then finish("max ticks") end
end

log("=== autorun_w5a_flag_watch ===")
log(string.format("flag=%d (0x%X) bank byte=0x%08X mask=0x%02X", FLAG, FLAG, BANK_BYTE, BANK_MASK))
log(string.format("sstate=%s boot_delay=%d post_frames=%d max_ticks=%d",
    SSTATE == "" and "(none)" or SSTATE, BOOT_DELAY, POST_FRAMES, MAX_TICKS))

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
