-- autorun_door_menu_refusal.lua
--
-- Why does the menu button open nothing after a Door of Light arrival on
-- map01? Runs the same Door use as autorun_door_item_use.lua (poke the item
-- into bag row 0, open the pause menu, Items, Use, row 0, confirm), then keeps
-- pressing the menu button on the destination and watches the field pad
-- controller FUN_801D01B0 (PROT 0897, base 0x801CE818) decide.
--
-- Observation (every BP guarded on the 0897 instruction word at its VA, so
-- the menu overlay's bytes at the same VA do not count):
--   * 0x801D01B0  controller entry: counted per vsync; the player's +0x10
--                 flags word (actor *0x8007C364) at entry.
--   * 0x801D0250  the menu-button leg (reached only with +0x10 & 0x80000
--                 clear and no overworld action press).
--   * 0x801D02D8  lock refusal: deny buzz 0x23 (_1F800394 & 0x08000000).
--   * 0x801D02E8  accept: cue 0x20 + menu actor spawn.
--   * 0x801D0334  skip-exit (engaged bit set, or no menu press).
--   * 0x801F1278  menu installer enter.
--   Per vsync: game mode 0x8007B83C, scene 0x80084548, player +0x10,
--   _1F800394, edge pad 0x8007B874, menu mask 0x800846D8, debug word
--   0x8007B98C, overworld flag 0x8007B6A8, held pad 0x8007B850, the
--   entry count. A row is written only when a field changes (entry count
--   excluded) or a press is scheduled.
--   * Optional screenshots at LEGAIA_SHOTS vsyncs ("1350,1450") ->
--     shot_<v>.raw/.meta (scripts/pcsx-redux/raw2png.py).
--
-- Output: <LEGAIA_OUT_DIR>/menu.log.
-- Env: LEGAIA_SSTATE / LEGAIA_FRAMES / LEGAIA_OUT_DIR (run_probe.sh),
--      LEGAIA_DOOR_POKES, LEGAIA_DOOR_SCRIPT (see autorun_door_item_use.lua),
--      LEGAIA_SHOTS, LEGAIA_WRITERS=1 arms a write watch on player +0x10
--      (logs pc + value whenever bit 0x80000 changes); LEGAIA_SCRIPTS=1
--      logs each actor script step of the script runner FUN_80039B7C.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")
local bp = require("probe.bp")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1800)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/door_menu_refusal")
local POKES = probe.getenv("LEGAIA_DOOR_POKES", "12:0x88")
local SCRIPT = probe.getenv("LEGAIA_DOOR_SCRIPT",
    "60:SELECT,200:CROSS,320:CROSS,440:CROSS,560:CROSS,1320:SELECT,1420:TRIANGLE,1520:SELECT,1620:SELECT")
local SHOTS = probe.getenv("LEGAIA_SHOTS", "")
local WRITERS = probe.getenv("LEGAIA_WRITERS", "") == "1"
local SCRIPTS = probe.getenv("LEGAIA_SCRIPTS", "") == "1"
-- "vsync:0xADDR": from that vsync, log every write to ADDR with pc / ra / the
-- caller chain's saved ra words.
local WATCH = probe.getenv("LEGAIA_WATCH", "")
os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/menu.log", "w")
local function log(s)
    PCSX.log("[menu] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local function u32(x) return (tonumber(x) or 0) % 0x100000000 end

local GP = 0x8007B318
local BAG = 0x80085958
local W = {
    entry = { 0x801D01B0, 0x3C028008 },
    leg = { 0x801D0250, 0x3C038008 },
    deny = { 0x801D02D8, 0x0C00D6F4 },
    accept = { 0x801D02E8, 0x0C00D6D4 },
    skip = { 0x801D0334, 0x24140008 },
    install = { 0x801F1278, 0x27BDFFD8 },
}
local function is_0897(k) return u32(probe.read_u32(W[k][1])) == W[k][2] end

local vs = 0
local entries = 0

local presses = {}
for at, name in string.gmatch(SCRIPT, "(%d+):(%u+)") do
    presses[#presses + 1] = { at = tonumber(at), btn = pad.BTN[name], name = name }
end
local shots = {}
for v in string.gmatch(SHOTS, "(%d+)") do shots[tonumber(v)] = true end
local HOLD = 8

local function scene_name()
    local out = {}
    for i = 0, 11 do
        local c = probe.read_u8(0x80084548 + i) or 0
        if c == 0 then break end
        out[#out + 1] = string.char(c)
    end
    return table.concat(out)
end

local function player() return u32(probe.read_u32(0x8007C364)) end
local function pflags()
    local p = player()
    if p >= 0x80000000 and p < 0x80200000 then return u32(probe.read_u32(p + 0x10)) end
    return 0
end

local function poke_item(ITEM)
    local lo = probe.read_u16(GP + 0x2D2) or 0
    probe.write_u8(BAG + lo * 2, ITEM)
    probe.write_u8(BAG + lo * 2 + 1, 1)
    log(string.format("v=%d poked item 0x%02X x1 into bag slot %d", vs, ITEM, lo))
end

local function shot(v)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ok and ss then
        local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
        local h = io.open(string.format("%s/shot_%d.raw", OUT_DIR, v), "wb")
        h:write(tostring(ss.data)); h:close()
        local m = io.open(string.format("%s/shot_%d.raw.meta", OUT_DIR, v), "w")
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", tonumber(ss.width), tonumber(ss.height), bpp))
        m:close()
        log(string.format("v=%d screenshot", v))
    end
end

local last_row = nil
local function sample()
    local row = string.format(
        "mode=%02X scene=%s player=%08X flags=%08X gate=%08X edge=%08X mask=%08X dbg=%08X ow=%d held=%08X",
        probe.read_u8(0x8007B83C) or 0, scene_name(), player(), pflags(),
        u32(probe.read_scratch_u32(0x1F800394)), u32(probe.read_u32(0x8007B874)),
        u32(probe.read_u32(0x800846D8)), u32(probe.read_u32(0x8007B98C)),
        probe.read_u8(0x8007B6A8) or 0, u32(probe.read_u32(0x8007B850)))
    if row ~= last_row then
        log(string.format("v=%d entries=%d %s", vs, entries, row))
        last_row = row
    end
end

local last_bit = nil
probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        bp.arm(W.entry[1], "Exec", 4, "ctl_entry", function()
            if not is_0897("entry") then return end
            entries = entries + 1
        end)
        bp.arm(W.leg[1], "Exec", 4, "ctl_menu_leg", function()
            if not is_0897("leg") then return end
            local e = u32(probe.read_u32(0x8007B874))
            if e ~= 0 then
                log(string.format("v=%d menu leg edge=%08X mask=%08X", vs, e,
                    u32(probe.read_u32(0x800846D8))))
            end
        end)
        bp.arm(W.deny[1], "Exec", 4, "ctl_deny", function()
            if is_0897("deny") then log(string.format("v=%d DENY buzz 0x23", vs)) end
        end)
        bp.arm(W.accept[1], "Exec", 4, "ctl_accept", function()
            if is_0897("accept") then log(string.format("v=%d ACCEPT menu", vs)) end
        end)
        bp.arm(W.install[1], "Exec", 4, "installer", function()
            if not is_0897("install") then return end
            local r = PCSX.getRegisters()
            log(string.format("v=%d FUN_801F1278 a0=%08X ra=%08X", vs, u32(r.GPR.n.a0), u32(r.GPR.n.ra)))
        end)
        if SCRIPTS then
            -- FUN_80039B7C's field-VM call (jal 0x801DE840 at 0x80039E14):
            -- s2 = the script actor, s3 = its program +0x90, s1 = its pc
            -- (a0 / a1 are still in their load / shift shadow here).
            -- One row per (actor, pc) change.
            local last = {}
            bp.arm(0x80039E14, "Exec", 4, "actor_script_step", function()
                local r = PCSX.getRegisters()
                local a = u32(r.GPR.n.s2)
                local prog = u32(r.GPR.n.s3)
                local pc = u32(r.GPR.n.s1) % 0x10000
                local key = string.format("%08X:%04X", prog, pc)
                if last[a] ~= key then
                    last[a] = key
                    log(string.format("v=%d script actor=%08X prog=%08X pc=%04X op=%s flags=%08X", vs, a,
                        prog, pc, probe.bytes_to_hex(probe.read_bytes(prog + pc, 12)),
                        u32(probe.read_u32(a + 0x10))))
                end
            end)
        end
        if WRITERS then
            -- player +0x10 write watch: re-armed when the player pointer moves.
            local armed_at = nil
            local function arm_watch()
                local p = player()
                if p < 0x80000000 or p == armed_at then return end
                armed_at = p
                bp.arm(p + 0x10, "Write", 4, "pflags_write", function()
                    local r = PCSX.getRegisters()
                    local f = pflags()
                    local bitv = bit.band(f, 0x80000) ~= 0
                    log(string.format("v=%d +0x10 write pc=%08X ra=%08X old=%08X", vs,
                        u32(r.pc), u32(r.GPR.n.ra), f))
                    last_bit = bitv
                end)
                log(string.format("v=%d armed +0x10 write watch at %08X", vs, p + 0x10))
            end
            probe.WRITE_REARM = arm_watch
        end
        return {}
    end,

    on_capture = function(_ctx, tick)
        vs = tick
        for at, addr in string.gmatch(WATCH, "(%d+):(0x%x+)") do
            if tick == tonumber(at) then
                local a = tonumber(addr)
                bp.arm(a, "Write", 4, "watch", function()
                    local r = PCSX.getRegisters()
                    log(string.format("v=%d watch %08X write pc=%08X ra=%08X a0=%08X a1=%08X a2=%08X v0=%08X old=%08X",
                        vs, a, u32(r.pc), u32(r.GPR.n.ra), u32(r.GPR.n.a0), u32(r.GPR.n.a1),
                        u32(r.GPR.n.a2), u32(r.GPR.n.v0), u32(probe.read_u32(a))))
                end)
                log(string.format("v=%d armed write watch %08X", tick, a))
            end
        end
        if probe.WRITE_REARM and (tick % 30 == 0) then probe.WRITE_REARM() end
        for at, id in string.gmatch(POKES, "(%d+):(0x%x+)") do
            if tick == tonumber(at) then poke_item(tonumber(id)) end
        end
        for _, s in ipairs(presses) do
            if tick == s.at then
                pad.force(s.btn)
                log(string.format("v=%d press %s", tick, s.name))
            elseif tick == s.at + HOLD then
                pad.release(s.btn)
            end
        end
        if shots[tick] then shot(tick) end
        for _, s in ipairs(presses) do
            if tick >= s.at and tick <= s.at + HOLD + 4 then
                log(string.format("v=%d  (press window) entries=%d flags=%08X edge=%08X", tick,
                    entries, pflags(), u32(probe.read_u32(0x8007B874))))
            end
        end
        sample()
        entries = 0
    end,

    on_done = function()
        for _, s in ipairs(presses) do pad.release(s.btn) end
        log("=== door_menu_refusal done ===")
        if LOG then LOG:close() end
    end,
})
