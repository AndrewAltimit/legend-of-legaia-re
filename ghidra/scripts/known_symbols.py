# known_symbols.py - curated (address, name, comment) table for the
# Ghidra label applier (apply_known_symbols.py).
#
# These are the SCUS_942.54-resident functions this project has pinned by hand
# (see docs/reference/functions.md). After a fresh import the executable comes
# in as a raw blob with every function anonymous (FUN_xxxxxxxx); running the
# applier re-attaches these names + one-line role comments in a single pass, so
# the asset/loader/CD/dispatch cluster is readable immediately.
#
# Scope: SCUS-resident only (0x80010000..0x8007C000). RAM overlays at
# 0x801C0000+ are deliberately excluded - the same overlay address holds
# different code per overlay, so naming by address alone would mislabel.
#
# Source of truth is our own clean-room analysis (no Sony/PsyQ SDK data).
# ASCII-only: the Ghidra-bundled Jython 2.7 chokes on non-ASCII source.

SCUS_LO = 0x80010000
SCUS_HI = 0x8007C000

# (address, symbol_name, one-line role comment)
SYMBOLS = [
    # --- asset loading + dispatch ---
    (0x8001A55C, "lzs_decode", "LZS decoder (4KB ring buffer init to zeros)"),
    (0x8001A8B0, "raw_memcpy", "Raw memcpy; asset dispatcher copy_only=1 path"),
    (0x8001E1B4, "per_stage_init", "Allocates the 0x62C00 asset buffer at DAT_8007B85C"),
    (0x8001F05C, "asset_type_dispatch", "Asset-type (type_size, copy_only) switch"),
    (0x8001FE70, "battle_init_tim_walker", "Battle-init scene_tmd_stream chunk walker"),
    (0x80020224, "descriptor_pair_walker", "Descriptor-pair walker (called from town overlay)"),
    (0x80020454, "actor_alloc", "Actor allocator (free-list LIFO at DAT_8007C348)"),
    (0x80020DE0, "actor_free", "Actor free (pairs with actor_alloc)"),
    (0x8002541C, "streaming_asset_driver", "Streaming-asset driver: tim.dat/move.mdt/DATA_FIELD"),
    (0x800255B8, "field_filename_loader", "Filename builder + loader (PROT/FIELD paths)"),
    (0x800268DC, "tmd_ptr_fixup", "TMD pointer fixup: object-table offsets -> absolute"),
    (0x80026B4C, "tmd_register", "TMD register: validates 0x80000002, stores at 0x8007C018+idx*4"),
    (0x8002735C, "tmd_render", "Legaia TMD renderer (GTE transform + addPrim)"),
    # --- move / table VMs (SCUS-resident entry points) ---
    (0x80023070, "move_vm", "Move-table opcode VM (71 ops, JT 0x80010778)"),
    (0x800204F8, "move_table_consumer", "Move-table consumer/parser"),
    # --- disc / loader chain ---
    (0x8001F7C0, "field_asset_loader", "Per-scene field-asset loader (dest, name, record)"),
    (0x8001FD44, "scene_change_packet", "Scene-change-packet API (name_ptr)"),
    (0x8001D424, "load_initmap", "Loads initmap.txt default start map into 0x8007050C"),
    (0x8001D7F8, "scene_name_sync", "Syncs scene name 0x8007050C -> active 0x80084548"),
    (0x8003D3C4, "iso_file_loader", "Path-based ISO9660 file loader (path, dest)"),
    (0x8003E360, "dual_mode_loader", "Dual-mode loader: retail ISO9660 vs debug PROT TOC"),
    (0x8003E4E8, "boot_toc_loader", "Boot TOC loader: PROT.DAT first 3 sectors -> 0x801C70F0"),
    (0x8003E6BC, "path_opener", "Path-based opener: dev path -> PROT index via CDNAME map"),
    (0x8003E800, "async_lba_loader", "Async LBA loader (dest, lba, flags)"),
    (0x8003E8A8, "prot_toc_resolver", "PROT TOC index resolver (index, flag) -> LBA"),
    (0x8003EB98, "byindex_sync_loader", "By-index sync loader (TOC resolve + read)"),
    (0x8003EBE4, "overlay_loader_a", "Overlay loader A: param+0x381 raw-TOC idx (= extraction entry param+0x37F)"),
    (0x8003EC70, "overlay_loader_b", "Overlay loader B (parallel; summon-magic overlays; same param+0x37F extraction idx)"),
    (0x8003EF14, "field_buffer_stream_poller", "Field-buffer per-sector streaming poller"),
    (0x8003F128, "async_cd_kickoff", "Async CD read kickoff"),
    (0x8005C328, "lba_to_msf", "LBA -> BCD-MSF converter"),
    (0x8005C42C, "msf_to_lba", "BCD-MSF -> LBA: ((m*60+s)*75+f)-150"),
    (0x8005D9A0, "cd_dma_read", "CD DMA-channel-3 synchronous read primitive"),
    (0x8005DBB4, "iso_dir_lookup", "ISO9660 directory lookup (out, filename)"),
    (0x8005E4D4, "sync_lba_file_read", "Synchronous LBA file reader (count, lba, dest)"),
    (0x8005E574, "streaming_read_irq_cb", "Streaming-read per-IRQ callback"),
    (0x8005E788, "streaming_read_start", "Streaming-read starter (registers IRQ cb)"),
    (0x8005E9A4, "streaming_read_api", "Public streaming-read API (count, dest, mode)"),
    # --- top-level mode dispatch ---
    (0x80017714, "main_mode_dispatch", "Main game-mode dispatcher (reads next-mode global)"),
    (0x80025B64, "mode2_field_init", "Mode 2 (field INIT) handler; New Game boot chain"),
]


def in_scus_range(addr):
    return SCUS_LO <= addr < SCUS_HI
