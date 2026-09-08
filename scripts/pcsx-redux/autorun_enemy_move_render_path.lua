-- autorun_enemy_move_render_path.lua
--
-- Which render path does an ENEMY battle action take?
--
-- The player Seru-magic summon was pinned by hit counters: the battle
-- per-actor draw FUN_80048A08 (35-64x/frame) -> the per-object rigid-TRS
-- keyframe decoder FUN_8004998C -> cluster-A FUN_80043390, with the move VM
-- FUN_80023070 at noise level and FUN_801F7088 never entered
-- (docs/subsystems/effect-vm.md). The ENEMY side of the same question - a
-- monster's special (Gimard's "Fire Tail") - was characterised only from
-- static mid-cast frames, which show one live move-VM part-actor in the pool
-- DAT_801C90F0 but cannot measure per-frame drivers.
--
-- This probe counts, per capture frame, hits on the five candidate drivers and
-- logs the battle-side discriminators alongside them, so the enemy action's
-- path is measured rather than inferred:
--
--   FUN_80048A08  battle per-actor draw          (the player-summon path)
--   FUN_8004998C  per-object rigid-TRS decoder   (its keyframe leg)
--   FUN_80023070  move VM                        (the part-actor scene graph)
--   FUN_80021DF4  generic SCUS actor tick        (what ticks a pooled part)
--   FUN_801F7088  slot-B alias (control: expect 0)
--
-- Per-frame columns also carry the loader-B current id (gp+0x934 =
-- 0x8007BC4C; 5 = the move-FX module PROT 0900, 8 = the Gimard player
-- stager PROT 0903), the battle ctx action-state byte and active-actor byte,
-- and the occupancy of the 0x60-slot move-VM part pool at DAT_801C90F0 - so a
-- frame in which a monster is mid-special is identifiable in the log without
-- a screenshot.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --lua scripts/pcsx-redux/autorun_enemy_move_render_path.lua \
--       --scenario rim_elm_gimard_victory --frames 600 \
--       --out-dir captures/w2c/enemy_move_render_path
--
-- Output: enemy_move_render_path.csv (one row per capture frame).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- Candidate per-frame drivers.
local BP = {
    { addr = 0x80048A08, name = "actor_draw" },   -- FUN_80048A08
    { addr = 0x8004998C, name = "trs_keyframe" }, -- FUN_8004998C
    { addr = 0x80023070, name = "move_vm" },      -- FUN_80023070
    { addr = 0x80021DF4, name = "actor_tick" },   -- FUN_80021DF4
    { addr = 0x801F7088, name = "slotb_alias" },  -- control
}

local CTX_PTR      = 0x8007BD24 -- -> battle ctx
local LOADER_B_ID  = 0x8007BC4C -- gp+0x934, slot-B current overlay id
local PART_POOL    = 0x801C90F0 -- 0x60 move-VM part-actor pointers
local PART_POOL_N  = 0x60

local frame_hits = {}
local total_hits = {}
for _, b in ipairs(BP) do
    frame_hits[b.name] = 0
    total_hits[b.name] = 0
end

local csv = probe.csv_open(probe.out_path("enemy_move_render_path.csv"),
    "frame,actor_draw,trs_keyframe,move_vm,actor_tick,slotb_alias," ..
    "loaderB,ctx_state,ctx_actor,parts_live,ctx_head")

local armed = false

local function make_cb(name)
    return function()
        frame_hits[name] = frame_hits[name] + 1
        total_hits[name] = total_hits[name] + 1
    end
end

local function arm()
    for _, b in ipairs(BP) do
        probe.arm_breakpoint(b.addr, "Exec", 4, b.name, make_cb(b.name))
    end
    PCSX.log("[enemy-path] armed " .. #BP .. " Exec breakpoints")
    armed = true
end

-- Count non-null pointers in the move-VM part pool.
local function parts_live()
    local n = 0
    for i = 0, PART_POOL_N - 1 do
        local p = probe.read_u32(PART_POOL + i * 4)
        if p and p ~= 0 then n = n + 1 end
    end
    return n
end

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 600),
    snapshot_path  = probe.out_path("enemy_move_render_path.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        if not armed and elapsed >= 2 then arm() end
        if not armed then return end
        local ctxp = probe.read_u32(CTX_PTR) or 0
        local state, actor = 0, 0
        local head = ""
        if ctxp ~= 0 then
            state = probe.read_u8(ctxp + 0x06) or 0
            actor = probe.read_u8(ctxp + 0x07) or 0
            -- ctx head bytes +0x04..+0x0B, so an action window is locatable in
            -- the log after the fact without re-running the capture.
            for i = 0x04, 0x0B do
                head = head .. string.format("%02X", probe.read_u8(ctxp + i) or 0)
            end
        end
        csv:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s",
            elapsed,
            frame_hits.actor_draw, frame_hits.trs_keyframe,
            frame_hits.move_vm, frame_hits.actor_tick, frame_hits.slotb_alias,
            probe.read_u32(LOADER_B_ID) or 0,
            state, actor, parts_live(), head)
        for _, b in ipairs(BP) do frame_hits[b.name] = 0 end
    end,
    on_done = function()
        csv:close()
        local parts = {}
        for _, b in ipairs(BP) do
            parts[#parts + 1] = string.format("%s=%d", b.name, total_hits[b.name])
        end
        PCSX.log("[enemy-path] done. totals: " .. table.concat(parts, " "))
    end,
})
