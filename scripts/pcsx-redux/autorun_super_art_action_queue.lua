-- autorun_super_art_action_queue.lua
--
-- Captures the per-actor BATTLE ACTION QUEUE for a player Super / Miracle Art
-- combo -- the interleaved art constants + combo-specific connector bytes.
--
-- This is the refined target after the ctx[+0x274] capture
-- (autorun_super_art_queue_builder.lua) showed that ctx+0x274 is the turn-
-- order active-actor field (written by recompute_battle_order FUN_801DABA4),
-- NOT the connector queue. The action queue is the per-actor
-- "action-parameter byte stream" at actor[+0x1DF..+0x1F2] (battle-action.md):
-- a Miracle Art clears the queue and writes its replacement string there; a
-- Super Art replaces the matched tail. The combo-specific connectors (Vahn's
-- 0x27 -> 0F in Tri-Somersault vs 0E in Power Slash) live in that stream.
--
-- The battle-actor pointer table is 0x801C9370 (8 entries x 4 bytes; slots
-- 0..2 = party). We resolve the three party actors post-load and (1) snapshot
-- each one's +0x1D8..+0x200 window so an already-built queue is visible
-- immediately, and (2) range-watch +0x1DF..+0x1F3 via probe.step.find_writer
-- so the build / dequeue writes are traced with PC + the post-write bytes.
--
-- Run on the Noa Miracle Art save (no input needed):
--   LEGAIA_FRAMES=1800 timeout --kill-after=20s 600s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario battle_noa_miracle_art_combo \
--       --lua scripts/pcsx-redux/autorun_super_art_action_queue.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
-- The snapshot is taken on the first post-load frame, so a small capture
-- budget is enough; the harness self-quit waits `capture_frames + quit_delay`
-- real vsyncs from capture-start, and the interpreter is slow (~a few fps), so
-- keep this low. The `timeout` wrapper still bounds the run either way.
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 60)
-- The combo queue is resident at load (the save is taken after the combo is
-- committed), so the per-actor +0x1DF snapshot at arm time IS the answer. The
-- `find_writer` dequeue trace arms ~30 write-breakpoints which slows the
-- interpreter to a crawl, so it is opt-in (LEGAIA_TRACE_WRITES=1).
local TRACE    = probe.getenv_num("LEGAIA_TRACE_WRITES", 0)
local OUT_PATH = probe.out_path("super_art_action_queue.csv")

local ACTOR_TABLE = 0x801C9370 -- 8 x u32 battle-actor pointers; 0..2 = party
local CTX_PTR     = 0x8007BD24 -- *(CTX_PTR)+0x274 = active-actor index
local Q_OFF       = 0x1DF      -- action-parameter byte stream head
local Q_LEN       = 0x14       -- +0x1DF..+0x1F2 (the stream) + a little slack
local SNAP_LO     = 0x1D8      -- snapshot window (incl. +0x1DD target, +0x1DE cat)
local SNAP_HI     = 0x200

local function party_actor(slot)
    local p = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
    return p % 0x100000000
end

local function snapshot(p)
    local b = probe.read_bytes(p + SNAP_LO, SNAP_HI - SNAP_LO)
    if b == nil then return "" end
    return probe.bytes_to_hex(b):gsub("%s+", "")
end

local csv = probe.csv_open(OUT_PATH, "tick,slot,pc,actor,queue_1df_hex")
local tick = 0
local handles = {}
local armed = false
local _quiet_prev = 0
local _quiet_frames = 0

local function arm_all(_hctx)
    if armed then return end
    -- Wait until at least party slot 0 resolves into RAM (post-load).
    local p0 = party_actor(0)
    if not probe.in_ram(p0) then return end
    armed = true
    local act = probe.read_u8((probe.read_u32(CTX_PTR) or 0) + 0x274)
    PCSX.log(string.format("[aq] armed; active-actor index (ctx+0x274)=%s", tostring(act)))
    for slot = 0, 2 do
        local p = party_actor(slot)
        if probe.in_ram(p) then
            local snap = snapshot(p)
            PCSX.log(string.format("[aq] party slot %d actor=0x%08X  +0x1D8..0x200=%s",
                slot, p, snap))
            csv:row("0,%d,0x00000000,0x%08X,%s", slot, p, snap)
            local s = slot
            if TRACE ~= 0 then
                handles[#handles + 1] = probe.step.find_writer(p + Q_OFF, Q_LEN, {
                    label = string.format("p%d", slot),
                    read_len = Q_LEN,
                    on_write = function(rg)
                        tick = tick + 1
                        local pc = (tonumber(rg.pc) or 0) % 0x100000000
                        local now = probe.read_bytes(p + Q_OFF, Q_LEN)
                        local hex = now and probe.bytes_to_hex(now):gsub("%s+", "") or "?"
                        csv:row("%d,%d,0x%08X,0x%08X,%s", tick, s, pc, p, hex)
                        PCSX.log(string.format("[aq] #%d slot%d pc=0x%08X actor+0x1DF=[%s]",
                            tick, s, pc, hex))
                    end,
                })
            end
        end
    end
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        PCSX.log("[aq] deferring arm until the save state loads (on_capture)")
        return {}
    end,

    -- Early-quit so the run doesn't idle out the full frame budget (which
    -- reads like a hang under the interpreter). Default (snapshot-only): the
    -- resident queue is captured at arm time, so quit ~30 frames later. With
    -- LEGAIA_TRACE_WRITES=1: keep running until the +0x1DF writes have stayed
    -- stable for ~1s, then quit.
    on_capture = function(hctx, _elapsed)
        arm_all(hctx)
        if not armed then return end
        _quiet_frames = _quiet_frames + 1
        if TRACE == 0 then
            if _quiet_frames >= 30 then hctx.request_quit = true end
            return
        end
        local total = 0
        for _, h in ipairs(handles) do total = total + h:count() end
        if total ~= _quiet_prev then
            _quiet_prev = total
            _quiet_frames = 0
        elseif total > 0 and _quiet_frames >= 60 then
            hctx.request_quit = true
        end
    end,

    on_done = function()
        csv:close()
        local total = 0
        for _, h in ipairs(handles) do total = total + h:count() end
        PCSX.log(string.format(
            "=== super-art action-queue probe: armed=%s queue writes=%d ===",
            tostring(armed), total))
        if not armed then
            PCSX.log("[aq] party actor table 0x801C9370 never resolved -- not a battle save?")
        elseif total == 0 then
            PCSX.log("[aq] no writes to actor+0x1DF -- the queue snapshots at tick 0 "
                .. "still show whatever was resident at load (the built combo, if any).")
        end
    end,
})
