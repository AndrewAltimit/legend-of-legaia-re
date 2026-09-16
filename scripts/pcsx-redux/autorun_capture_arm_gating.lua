-- autorun_capture_arm_gating.lua
--
-- Per-arm frame gating for the **capture-class** half of the slot-B cast
-- band - the fourteen trampoline-reached tick bodies of PROT 0940 / 0941 /
-- 0943 / 0944 / 0950 / 0956 / 0962 that `legaia_engine_vm::cast_arm_ticks`
-- ports without their dwell.
--
-- The sister probe `autorun_w3a_cast_oracle.lua` measures the player-Seru
-- half by rewriting a PARTY seat's queued action. That does not reach these
-- bodies: their action ids are enemy casts, so the caster has to be a
-- monster seat. This probe converts the monster's already-rolled action
-- instead (`+0x1DE = 2` Magic, `+0x1DF = <action id>`, `+0x1DD = <target>`),
-- which is the same conversion `autorun_special_cast_enemy_control.lua`
-- performs, plus the per-tick instrumentation the dwell needs.
--
-- ## The tick clock is the dispatcher, not a VSync
--
-- The dwell unit is **module ticks** (one entry into the body per battle-SM
-- pass), not VSyncs: the battle SM does not advance once per VSync, so a
-- VSync dwell carries host timing. The capture-class band has exactly one
-- tick dispatcher - `FUN_801F2160` in PROT 0898, called from the single site
-- `0x801E50C8` - so an Exec breakpoint there is one hit per module tick by
-- construction, whichever body is resident. The phase byte read AT that
-- entry is the arm about to run.
--
-- The twelve distinct body entry VAs are armed too, so each row also carries
-- which body ran and the `ra` it was called through - i.e. the body identity
-- recovered from EXECUTION rather than from a dump's printed address.
--
-- Outputs (probe.out_path, i.e. --out-dir):
--   ticks.csv     one row per module tick: phase, ctx scratch, the module's
--                 own countdown word, the scratchpad frame-step pair, the
--                 body VA entered this tick and its return address.
--   wrappers.csv  one row per FUN_801DD0AC / 4B0 / 6B4 call and return.
--   flow.csv      one row per battle-SM state / phase transition.
--
-- Env:
--   LEGAIA_SPELL         action id to force (default 0x51 Steal, PROT 0941)
--   LEGAIA_MONSTER_SEAT  actor-table seat of the caster (default 3)
--   LEGAIA_TARGET_SEAT   value written to caster +0x1DD (default 0)
--   LEGAIA_CD_VA         module countdown word; 0 = look up from LEGAIA_SPELL
--   LEGAIA_INJECT_AT     vsync to convert the monster's action on (default 20)
--   LEGAIA_PRESS_UNTIL   keep tapping CROSS until this vsync (default 400)
--   LEGAIA_MON_HP        monster HP + max HP to seed (default 9999)
--   LEGAIA_PARTY_HP      party HP + displayed HP to seed (default 9999)
--   LEGAIA_TAIL          vsyncs to keep logging after the band ends (60)
--   LEGAIA_IDLE_QUIT     quit after this many tickless vsyncs post-cast (240)
--   LEGAIA_LABEL         free-text label written into manifest.txt
--   LEGAIA_TRAP_UNMAPPED 1 = install the emulator's `UnknownMemoryRead` /
--                        `UnknownMemoryWrite` Lua hooks and log every
--                        unmapped access (pc, ra, address, width, phase) to
--                        faults.csv instead of letting the debugger PAUSE the
--                        emulator on it. Under `-debugger` an unmapped read
--                        stops the whole run with "8-bit read from unknown
--                        address" as the last log line and no PC - which is
--                        the shape the two faulting arms took. The hook
--                        answers 0 so the walk continues past the read.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local function env_addr(name)
    -- Address env vars are read with `tonumber`, which accepts the `0x` form
    -- directly - pass them as hex. A hand-converted decimal answers 0 rather
    -- than failing, which reads as "the code never ran".
    local v = tonumber(os.getenv(name) or "")
    if v == nil then return 0 end
    return v
end

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 2400)
local SPELL       = probe.getenv_num("LEGAIA_SPELL", 0x51)
local MON_SEAT    = probe.getenv_num("LEGAIA_MONSTER_SEAT", 3)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 0)
local INJECT_AT   = probe.getenv_num("LEGAIA_INJECT_AT", 20)
local PRESS_UNTIL = probe.getenv_num("LEGAIA_PRESS_UNTIL", 400)
local MON_HP      = probe.getenv_num("LEGAIA_MON_HP", 9999)
local PARTY_HP    = probe.getenv_num("LEGAIA_PARTY_HP", 9999)
local TAIL        = probe.getenv_num("LEGAIA_TAIL", 60)
local IDLE_QUIT   = probe.getenv_num("LEGAIA_IDLE_QUIT", 240)
local LABEL       = probe.getenv("LEGAIA_LABEL", "capture-arm")
local TRAP        = probe.getenv("LEGAIA_TRAP_UNMAPPED", "") == "1"

local ACTOR_TABLE = 0x801C9370
local CTX_PTR     = 0x8007BD24
local SLOTB       = 0x801F69D8
local LOADER_ID   = 0x8007BC4C
local CAP_TICK_TABLE = 0x801CF56C   -- 32-slot capture-class arm table (0898)
local CAP_DISPATCH   = 0x801F2160   -- FUN_801F2160, one hit per module tick
local SPELL_TABLE    = 0x800754C8   -- static spell table, 12-byte stride

-- The per-module countdown word each body draws down by the scratchpad frame
-- step. Addresses from `legaia_engine_vm::cast_arm_ticks`' disclosure list;
-- keyed here by action id so a run needs only LEGAIA_SPELL.
local MODULE_CD = {
    [0x3C] = 0x801F864C, [0x50] = 0x801F864C, [0xAC] = 0x801F864C, [0xAE] = 0x801F864C,
    [0x51] = 0x801F83EC, [0xB9] = 0x801F83EC,
    [0x40] = 0x801F7A04, [0xB5] = 0x801F7A04,
    [0x37] = 0x801F8360, [0x53] = 0x801F8360,
    [0x5A] = 0x801F86B0, [0xAB] = 0x801F86B0,
    [0x71] = 0x801F86A0, [0x75] = 0x801F86A0,
    [0xA2] = 0x801F89AC, [0xA3] = 0x801F89AC, [0xA4] = 0x801F89AC, [0xA5] = 0x801F89AC,
}

-- The twelve distinct body entry VAs the fourteen arms use. Only the resident
-- module's own body can fire, so arming all of them costs nothing and tells
-- the run which one ran without trusting the static table.
local BODY_VAS = {
    0x801F6A04, 0x801F6A24, 0x801F6D54, 0x801F6EF4, 0x801F7240, 0x801F7298,
    0x801F730C, 0x801F7470, 0x801F74A0, 0x801F78B8, 0x801F79F8, 0x801F7AE4,
}

-- Frame-step cells the countdown is drawn down by (scratchpad, not main RAM).
local SCR_STEP_A = 0x1F80037D
local SCR_STEP_B = 0x1F800393

local WRAPPERS = {
    { name = "DD0AC", entry = 0x801DD0AC, exit = 0x801DD4A8 },
    { name = "DD4B0", entry = 0x801DD4B0, exit = 0x801DD6AC },
    { name = "DD6B4", entry = 0x801DD6B4, exit = 0x801DD85C },
}

local CD_VA = env_addr("LEGAIA_CD_VA")
if CD_VA == 0 then CD_VA = MODULE_CD[SPELL] or 0 end

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end

local function ctxp()
    local c = u32(CTX_PTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end
local function actor(slot)
    local p = u32(ACTOR_TABLE + slot * 4)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function regs()
    local r = PCSX.getRegisters()
    return (r.GPR and r.GPR.n) or {}
end

local ticks_csv, wrap_csv, flow_csv, faults_csv
local n_faults = 0
local injected, band_seen = false, false
local tick_n, wrapper_n = 0, 0
local elapsed_now = 0
local last_tick_vsync = -1
local band_vsync = -1
local last_flow = ""
local pending = nil        -- the dispatcher row awaiting its body BP
local pending_w = {}
local body_hits = {}       -- body VA -> hit count
local quit_at = -1

local function flush_pending()
    if pending == nil then return end
    ticks_csv:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,0x%08X,0x%08X,0x%02X,%d,0x%08X",
        pending.n, pending.vsync, pending.phase, pending.c278, pending.c27a,
        pending.c0d, pending.c1a, pending.c6d8, pending.cd,
        pending.step, pending.body, pending.ra, pending.st7,
        pending.loader, pending.slotb0)
    pending = nil
end

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format(
            "== capture-class arm gating == label=%s spell=0x%02X caster_seat=%d target=%d cd=0x%08X",
            LABEL, SPELL, MON_SEAT, TARGET_SEAT, CD_VA))
        probe.env.write_manifest("autorun_capture_arm_gating.lua", {
            label = LABEL, sstate = SSTATE_PATH, spell = string.format("0x%02X", SPELL),
            monster_seat = MON_SEAT, target_seat = TARGET_SEAT, frames = FRAMES,
            cd_va = string.format("0x%08X", CD_VA),
        })
        ticks_csv = probe.csv_open(probe.out_path("ticks.csv"),
            "tick,vsync,phase,c278,c27a,c0d,c1a,c6d8,cd,step,body,ra,st7,loader,slotb0")
        wrap_csv = probe.csv_open(probe.out_path("wrappers.csv"),
            "tick,vsync,kind,which,a0,a1,a2,a3,ra,v0,phase")
        flow_csv = probe.csv_open(probe.out_path("flow.csv"),
            "vsync,ctx6,ctx7,acting,phase,mon_cat,mon_id,mon_tgt,loader,arm,slotb0")

        if TRAP then
            -- The emulator calls these globals for any access to an address
            -- no device backs. Logging the CPU state here names the
            -- instruction that formed the pointer - the one thing the paused
            -- run could not say.
            faults_csv = probe.csv_open(probe.out_path("faults.csv"),
                "n,vsync,tick,kind,width,addr,value,pc,ra,st7,phase,loader,slotb0")
            local function fault_row(kind, addr, size, value)
                n_faults = n_faults + 1
                if n_faults > 500 then return end
                local n = regs()
                local r = PCSX.getRegisters()
                local cx = ctxp()
                faults_csv:row("%d,%d,%d,%s,%d,0x%08X,0x%08X,0x%08X,0x%08X,0x%02X,%d,%d,0x%08X",
                    n_faults, elapsed_now, tick_n, kind, size, tou32(addr), tou32(value),
                    tou32(r.pc), tou32(n.ra), cx and u8(cx + 7) or 0,
                    cx and u8(cx + 0x279) or 0, u8(LOADER_ID), u32(SLOTB))
                if n_faults <= 8 then
                    PCSX.log(string.format("[unmapped %s%d] t%d addr=0x%08X pc=0x%08X ra=0x%08X",
                        kind, size, elapsed_now, tou32(addr), tou32(r.pc), tou32(n.ra)))
                end
            end
            _G.UnknownMemoryRead = function(addr, size)
                fault_row("read", addr, size, 0)
                return 0
            end
            _G.UnknownMemoryWrite = function(addr, size, value)
                fault_row("write", addr, size, value)
                return true
            end
        end

        -- One hit per capture-class module tick.
        probe.arm_breakpoint(CAP_DISPATCH, "Exec", 4, "cap_dispatch", function()
            local cx = ctxp()
            if cx == nil then return end
            flush_pending()
            tick_n = tick_n + 1
            last_tick_vsync = elapsed_now
            local sa = probe.mem.read_scratch_u8(SCR_STEP_A)
            local sb = probe.mem.read_scratch_u8(SCR_STEP_B)
            pending = {
                n = tick_n, vsync = elapsed_now,
                phase = u8(cx + 0x279), c278 = u8(cx + 0x278), c27a = u8(cx + 0x27A),
                c0d = u8(cx + 0x0D), c1a = u8(cx + 0x1A), c6d8 = u8(cx + 0x6D8),
                cd = CD_VA ~= 0 and u32(CD_VA) or -1,
                step = sa * sb, body = 0, ra = 0, st7 = u8(cx + 7),
                loader = u8(LOADER_ID), slotb0 = u32(SLOTB),
            }
        end)

        for _, va in ipairs(BODY_VAS) do
            body_hits[va] = 0
            probe.arm_breakpoint(va, "Exec", 4, string.format("body%08X", va), function()
                body_hits[va] = body_hits[va] + 1
                if pending ~= nil and pending.body == 0 then
                    pending.body = va
                    pending.ra = tou32(regs().ra)
                end
            end)
        end

        for _, w in ipairs(WRAPPERS) do
            local nm = w.name
            probe.arm_breakpoint(w.entry, "Exec", 4, "w_" .. nm, function()
                local n = regs()
                local cx = ctxp()
                wrapper_n = wrapper_n + 1
                pending_w[nm] = { a0 = tou32(n.a0), a1 = tou32(n.a1),
                                  a2 = tou32(n.a2), a3 = tou32(n.a3), ra = tou32(n.ra) }
                wrap_csv:row("%d,%d,call,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,,%d",
                    tick_n, elapsed_now, nm, pending_w[nm].a0, pending_w[nm].a1,
                    pending_w[nm].a2, pending_w[nm].a3, pending_w[nm].ra,
                    cx and u8(cx + 0x279) or -1)
            end)
            probe.arm_breakpoint(w.exit, "Exec", 4, "x_" .. nm, function()
                local n = regs()
                local p = pending_w[nm]
                local cx = ctxp()
                wrap_csv:row("%d,%d,ret,%s,%s,%s,%s,%s,%s,0x%08X,%d",
                    tick_n, elapsed_now, nm,
                    p and string.format("0x%08X", p.a0) or "",
                    p and string.format("0x%08X", p.a1) or "",
                    p and string.format("0x%08X", p.a2) or "",
                    p and string.format("0x%08X", p.a3) or "",
                    p and string.format("0x%08X", p.ra) or "",
                    tou32(n.v0), cx and u8(cx + 0x279) or -1)
                pending_w[nm] = nil
            end)
        end
        return {}
    end,

    on_capture = function(c, elapsed)
        elapsed_now = elapsed
        local cx = ctxp()
        local mon = actor(MON_SEAT)
        if cx == nil or mon == nil then return end

        probe.pad_release(probe.BTN.CROSS)
        if not band_seen and elapsed < PRESS_UNTIL then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end

        -- Convert the monster's already-rolled action before Begin is
        -- confirmed. The party seats keep whatever they queued; only the
        -- caster seat is rewritten, so the cast that runs is a retail cast
        -- with its retail caster kind.
        if not injected and elapsed >= INJECT_AT then
            injected = true
            if MON_HP > 0 then
                probe.write_u16(mon + 0x14C, MON_HP)
                probe.write_u16(mon + 0x14E, MON_HP)
            end
            -- +0x172 is the DISPLAYED HP the party HUD ramps towards, and the
            -- 0x51 exit gate holds the whole action band while a PARTY
            -- target's +0x14C differs from it. Seed both or the battle parks.
            for s = 0, 2 do
                local a = actor(s)
                if a ~= nil and PARTY_HP > 0 then
                    probe.write_u16(a + 0x14C, PARTY_HP)
                    probe.write_u16(a + 0x172, PARTY_HP)
                    probe.write_u16(a + 0x14E, PARTY_HP)
                end
            end
            probe.write_u8(mon + 0x1DE, 2)
            probe.write_u8(mon + 0x1DF, SPELL)
            probe.write_u8(mon + 0x1DD, TARGET_SEAT)
            local rec = SPELL_TABLE + SPELL * 12
            local sub_id = u8(rec + 1)
            PCSX.log(string.format(
                "[inject t%d] seat%d +0x1DE=2 +0x1DF=0x%02X +0x1DD=%d class=0x%02X sub=%d -> PROT %d arm=0x%08X",
                elapsed, MON_SEAT, SPELL, TARGET_SEAT, u8(rec), sub_id, 935 + sub_id,
                u32(CAP_TICK_TABLE + sub_id * 4)))
        end

        local st7 = u8(cx + 7)
        local ph = u8(cx + 0x279)
        if st7 == 0x70 or (st7 >= 0x6E and st7 <= 0x71) then
            if not band_seen then band_vsync = elapsed end
            band_seen = true
        end

        local rec = SPELL_TABLE + SPELL * 12
        local line = string.format("%02X/%02X/%d/%d/%02X/%02X/%d",
            u8(cx + 6), st7, u8(cx + 0x13), ph, u8(mon + 0x1DE), u8(mon + 0x1DF), u8(LOADER_ID))
        if line ~= last_flow then
            last_flow = line
            flow_csv:row("%d,0x%02X,0x%02X,%d,%d,%d,0x%02X,%d,%d,0x%08X,0x%08X",
                elapsed, u8(cx + 6), st7, u8(cx + 0x13), ph,
                u8(mon + 0x1DE), u8(mon + 0x1DF), u8(mon + 0x1DD), u8(LOADER_ID),
                u32(CAP_TICK_TABLE + u8(rec + 1) * 4), u32(SLOTB))
        end

        -- Quit once the band has run and the tick stream has gone quiet.
        if band_seen and last_tick_vsync >= 0 and quit_at < 0
            and elapsed > last_tick_vsync + IDLE_QUIT then
            quit_at = elapsed + TAIL
        end
        -- ... and quit the same way when the band was entered and NO tick ever
        -- arrived. Without this arm the run has no stop condition at all and
        -- sits until the wall-clock timeout kills it, which is the shape a
        -- cast that faults on entry takes: `ctx[7]` reaches the band, the
        -- dispatcher never runs, and a probe waiting on "the ticks stopped"
        -- waits for ticks that never started.
        if band_seen and last_tick_vsync < 0 and quit_at < 0
            and band_vsync >= 0 and elapsed > band_vsync + IDLE_QUIT then
            quit_at = elapsed + TAIL
            PCSX.log(string.format("[capture-arm] band entered at t%d, ZERO ticks by t%d",
                band_vsync, elapsed))
        end
        if quit_at >= 0 and elapsed >= quit_at then c.request_quit = true end
    end,

    on_done = function()
        flush_pending()
        PCSX.log(string.format("[capture-arm] spell=0x%02X ticks=%d wrappers=%d band=%s",
            SPELL, tick_n, wrapper_n, tostring(band_seen)))
        for _, va in ipairs(BODY_VAS) do
            if body_hits[va] > 0 then
                PCSX.log(string.format("[capture-arm] body 0x%08X entered %d times", va, body_hits[va]))
            end
        end
        if TRAP then
            PCSX.log(string.format("[capture-arm] unmapped accesses trapped: %d", n_faults))
        end
        if ticks_csv then ticks_csv:close() end
        if wrap_csv then wrap_csv:close() end
        if flow_csv then flow_csv:close() end
        if faults_csv then faults_csv:close() end
    end,
})
