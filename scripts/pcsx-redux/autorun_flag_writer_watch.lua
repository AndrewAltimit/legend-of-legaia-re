-- autorun_flag_writer_watch.lua
--
-- Story-flag WRITER hunt for flags whose script census is empty/noise
-- (the presumed "direct code path" writers - 0x482 Drake mist-wall,
-- 0x63A vell/vozz). Complements autorun_flag_reader_watch.lua: that
-- probe is a human-navigated firehose; this one is UNATTENDED - it
-- loads a bracketing save state, mashes X to advance the beat, and
-- watches the target flag BYTES for writes until the bit flips.
--
-- Bank math (docs/subsystems/script-vm.md, FUN_8003CE08/_CE34/_CE64):
--   byte = 0x80085758 + (idx >> 3);  mask = 0x80 >> (idx & 7)
--   0x482 -> byte 0x800857E8 mask 0x20
--   0x63A -> byte 0x8008581F mask 0x20
--
-- WATCHES:
--   1. Write-watch (width 1) on each distinct target byte - catches
--      helper-path AND direct/inlined stores. The BP fires MID-store,
--      so the hit row carries the pre-store value and a vsync-drained
--      "now=" committed value (the reader-watch's documented trap).
--      Full register context for every hit goes to the .detail.txt.
--   2. Exec-bp on FUN_8003CE08 (SET) / FUN_8003CE34 (CLEAR) - names
--      the helper-path caller (a0 = flag, ra = writer) so a byte hit
--      can be attributed helper-vs-direct.
--   3. Per-vsync bit poll on each target - logs the exact tick the bit
--      commits, banks a snapshot sstate at that moment, then keeps
--      capturing LEGAIA_QUIT_AFTER frames and self-quits.
--   4. Overlay-residency rows (csum of 512 bytes at each runtime slot
--      base) on scene/mode change - resolves slot-A VA aliasing when a
--      hit ra lands in 0x801C0000+ (join offline via
--      scripts/pcsx-redux/attribute_overlay_hits.py / static-overlays.toml).
--
-- PAD: edge-only CROSS mash (press 2 / release 10) to advance
-- post-battle dialogs + cutscenes headlessly. LEGAIA_MASH=0 disables
-- for human-driven runs.
--
-- Lua BPs are DEAD under --fast; run the default -interpreter
-- -debugger tier. The probe self-quits (exit 0 = a target bit flipped,
-- 2 = frame budget exhausted) but STILL wrap in `timeout --kill-after`.
--
-- Launch:
--   LEGAIA_SSTATE=saves/library/pcsx-redux/<fp>.sstate \
--   LEGAIA_FLAG=0x482,0x63A LEGAIA_FRAMES=36000 \
--   timeout --kill-after=30s 5400s \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_flag_writer_watch.lua
--
-- Output:
--   flag_writer_watch.csv   tick,kind,flag,pc,ra,mode,scene,count,note
--     kind = write   (byte write-watch hit; flag = representative target
--                     on that byte; note = "byte=0x.. pre=0x.. now=0x..")
--          | set | clear  (helper exec-bp; flag = a0, ra = caller)
--          | commit  (per-vsync poll saw the target bit flip 0->1)
--          | scene | mode | overlay  (context timeline)
--   flag_writer_watch.detail.txt   register context per write hit
--   commit_f<idx>.sstate           snapshot at the commit tick

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe   = require("probe")
local mem     = require("probe.mem")
local bp      = require("probe.bp")
local bit     = require("bit")
local pad     = require("probe.pad")
local sstate  = require("probe.sstate")
local version = require("probe.version")

local GAME_MODE  = 0x8007B83C
local SCENE_NAME = 0x8007050C
local FLAG_BASE  = 0x80085758
local FLAG_SET_PC   = 0x8003CE08
local FLAG_CLEAR_PC = 0x8003CE34
local OVERLAY_BASES = { 0x801CE818, 0x801F69D8 }
local OVERLAY_CSUM_BYTES = 512

local function parse_int(s)
    if s == nil or s == "" then return nil end
    if s:lower():sub(1, 2) == "0x" then return tonumber(s:sub(3), 16) end
    return tonumber(s)
end

local TARGETS, TARGET_LIST = {}, {}
do
    local spec = probe.getenv("LEGAIA_FLAG", "0x482,0x63A")
    for tok in spec:gmatch("[^,%s]+") do
        local n = parse_int(tok)
        if n ~= nil and not TARGETS[n] then
            TARGETS[n] = true
            TARGET_LIST[#TARGET_LIST + 1] = n
        end
    end
end

local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local MAX_FRAMES = probe.getenv_num("LEGAIA_FRAMES", 36000)
local QUIT_AFTER = probe.getenv_num("LEGAIA_QUIT_AFTER", 600)
local MASH       = probe.getenv("LEGAIA_MASH", "1") ~= "0"
local MASH_PRESS, MASH_STEP = 2, 12

local CSV = probe.csv_open(probe.out_path("flag_writer_watch.csv"),
    "tick,kind,flag,pc,ra,mode,scene,count,note")
local DETAIL = probe.out_path("flag_writer_watch.detail.txt")

local function u8(addr)  return mem.read_u8(addr)  or 0 end
local function u32v(v)
    v = bit.band(tonumber(v) or 0, 0xFFFFFFFF)
    if v < 0 then v = v + 4294967296 end
    return v
end
local function scene_name()
    local s = ""
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7F then break end
        s = s .. string.char(b)
    end
    return (s == "") and "?" or s
end
local function fnv1a32(s)
    local h = 0x811C9DC5
    for i = 1, #s do
        h = bit.bxor(h, s:byte(i)) % 4294967296
        local lo = bit.band(h, 0xFFFF)
        local hi = bit.rshift(h, 16) % 0x10000
        h = (lo * 0x01000193 + bit.band(hi * 0x01000193, 0xFFFF) * 0x10000)
            % 4294967296
    end
    return h
end
local function flag_byte(idx) return FLAG_BASE + bit.rshift(idx, 3) end
local function flag_mask(idx) return bit.rshift(0x80, bit.band(idx, 7)) end
local function flag_set(idx)
    return bit.band(u8(flag_byte(idx)), flag_mask(idx)) ~= 0
end

local vsync, loaded_at, armed = 0, nil, false
local version_pass, capture_disabled = false, false
local counts = {}
local pending = {}          -- queued rows from bp callbacks
local committed = {}        -- idx -> true once the bit is seen SET
local quit_countdown = nil
local any_write_hit = false
local last_scene, last_mode = nil, nil
local overlay_csums = {}
local forced_btn = nil

local function log(s) PCSX.log("[fwriter] " .. s) end

local function row(kind, flag, pc, ra, note)
    local key = string.format("%s|%d|%08X", kind, flag, ra)
    local n = (counts[key] or 0) + 1
    counts[key] = n
    CSV:row("%d,%s,%d,0x%08X,0x%08X,0x%02X,%s,%d,%s",
        vsync, kind, flag, pc, ra, u8(GAME_MODE), scene_name(), n, note or "")
end

local function poll_overlays()
    for _, base in ipairs(OVERLAY_BASES) do
        local win = mem.read_bytes(base, OVERLAY_CSUM_BYTES)
        if win ~= nil then
            local c = fnv1a32(tostring(win))
            if overlay_csums[base] ~= c then
                overlay_csums[base] = c
                row("overlay", 0, base, 0, string.format("csum=%08x", c))
            end
        end
    end
end

local function pad_set(btn)
    if forced_btn == btn then return end
    if forced_btn ~= nil then pad.release(forced_btn) end
    if btn ~= nil then pad.force(btn) end
    forced_btn = btn
end

local function arm_all()
    -- 1. Write-watch per distinct target byte.
    local byte_rep = {}
    for _, f in ipairs(TARGET_LIST) do
        local addr = flag_byte(f)
        if byte_rep[addr] == nil or f < byte_rep[addr] then
            byte_rep[addr] = f
        end
    end
    for addr, f in pairs(byte_rep) do
        bp.arm(addr, "Write", 1, string.format("fw_%X", f), function()
            local r  = PCSX.getRegisters()
            local pc = u32v(r.pc)
            local ra = u32v(r.GPR.n.ra)
            local pre = u8(addr)
            any_write_hit = true
            pending[#pending + 1] = {
                kind = "write", flag = f, pc = pc, ra = ra,
                addr = addr, pre = pre,
                detail = probe.capture_call_context(string.format(
                    "write byte=0x%08X (flag 0x%X's byte) pc=0x%08X ra=0x%08X tick=%d scene=%s",
                    addr, f, pc, ra, vsync, scene_name())),
            }
        end)
    end
    -- 2. Helper exec-bps: name helper-path writers (all flags, deduped
    --    by the CSV count column; target flags called out in the note).
    local function arm_helper(hpc, kind)
        bp.arm(hpc, "Exec", 4, "fwh_" .. kind, function()
            local r  = PCSX.getRegisters()
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x10000
            local ra = u32v(r.GPR.n.ra)
            pending[#pending + 1] = {
                kind = kind, flag = a0, pc = hpc, ra = ra,
                note = TARGETS[a0] and "tgt" or "",
            }
        end)
    end
    arm_helper(FLAG_SET_PC, "set")
    arm_helper(FLAG_CLEAR_PC, "clear")
    poll_overlays()
    armed = true
    local tl = {}
    for _, f in ipairs(TARGET_LIST) do
        tl[#tl + 1] = string.format("0x%X@0x%08X&0x%02X(%s)", f, flag_byte(f),
            flag_mask(f), flag_set(f) and "SET" or "clear")
    end
    log("armed: " .. table.concat(tl, " "))
    probe.write_manifest("autorun_flag_writer_watch.lua", {
        targets    = table.concat(tl, " "),
        sstate     = SSTATE,
        max_frames = tostring(MAX_FRAMES),
        quit_after = tostring(QUIT_AFTER),
        mash       = tostring(MASH),
        armed_tick = tostring(vsync),
        armed_scene = scene_name(),
    })
end

local function drain_pending()
    if #pending == 0 then return end
    for i = 1, #pending do
        local ev = pending[i]
        local note = ev.note or ""
        if ev.kind == "write" then
            note = string.format("byte=0x%08X pre=0x%02X now=0x%02X",
                ev.addr, ev.pre, u8(ev.addr))
        end
        row(ev.kind, ev.flag, ev.pc, ev.ra, note)
        if ev.detail then probe.append_call_context(DETAIL, ev.detail) end
        if ev.kind == "write" then
            log(string.format("WRITE byte 0x%08X pc=0x%08X ra=0x%08X %s",
                ev.addr, ev.pc, ev.ra, note))
        end
    end
    pending = {}
end

local function check_version_gate()
    if version_pass then return true end
    if version.record_mode() then
        capture_disabled = true
        return false
    end
    local ok, msg, terminal = version.check(version.USA_FINGERPRINT)
    if ok then
        version_pass = true
        log("version guard: " .. msg)
        return true
    end
    if terminal then
        log("FATAL version guard: " .. msg)
        capture_disabled = true
    end
    return false
end

local function on_vsync()
    vsync = vsync + 1
    if capture_disabled then return end

    if loaded_at == nil then
        if vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then
                log("FATAL: could not load save state " .. SSTATE)
                loaded_at = -1
                return
            end
            loaded_at = vsync
            log(string.format("state loaded at tick %d; mode=0x%02X scene=%s",
                vsync, u8(GAME_MODE), scene_name()))
        end
        return
    end
    if loaded_at < 0 then return end
    if not version_pass and not check_version_gate() then return end
    if not armed then arm_all() end

    drain_pending()

    local sc = scene_name()
    if sc ~= last_scene then
        last_scene = sc
        row("scene", 0, 0, 0, "")
        log(string.format("scene -> %s (tick %d)", sc, vsync))
        poll_overlays()
    end
    local md = u8(GAME_MODE)
    if md ~= last_mode then
        last_mode = md
        row("mode", md, 0, 0, "")
        poll_overlays()
    end

    -- 3. Commit poll: the moment a target bit reads SET.
    for _, f in ipairs(TARGET_LIST) do
        if not committed[f] and flag_set(f) then
            committed[f] = true
            row("commit", f, 0, 0, string.format("byte=0x%08X", flag_byte(f)))
            log(string.format("COMMIT: flag 0x%X is now SET (tick %d scene %s)",
                f, vsync, sc))
            sstate.save(probe.out_path(string.format("commit_f%X.sstate", f)))
            if quit_countdown == nil then quit_countdown = QUIT_AFTER end
        end
    end

    if MASH then
        local sub = (vsync - loaded_at) % MASH_STEP
        pad_set(sub < MASH_PRESS and pad.BTN.CROSS or nil)
    end

    if quit_countdown ~= nil then
        quit_countdown = quit_countdown - 1
        if quit_countdown <= 0 then
            drain_pending()
            pad_set(nil)
            log("done: target committed; quitting")
            CSV.fh:flush()
            PCSX.quit(0)
            return
        end
    end
    if (vsync - loaded_at) >= MAX_FRAMES then
        drain_pending()
        pad_set(nil)
        log(string.format("frame budget exhausted (%d); write_hits=%s",
            MAX_FRAMES, tostring(any_write_hit)))
        CSV.fh:flush()
        PCSX.quit(2)
        return
    end
    if (vsync % 600) == 0 then
        log(string.format("alive tick=%d mode=0x%02X scene=%s", vsync, md, sc))
        CSV.fh:flush()
    end
end

log("=== autorun_flag_writer_watch (unattended flag-writer hunt) ===")
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log("vsync listener installed")
