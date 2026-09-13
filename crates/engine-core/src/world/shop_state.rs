//! Town shop + prize-exchange session state (the gold shop and the casino / fishing prize counter).
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Town shop + prize-exchange session state (the gold shop and the casino / fishing prize counter).
pub struct ShopState {
    /// Item-table data the gold-shop path needs from `SCUS_942.54` (per-id buy
    /// price + a "names a real item" mask). Installed once at boot by the host
    /// (e.g. `BootSession`); `None` on disc-free builds, which leaves shop stock
    /// host-supplied and unpriced. See [`crate::shop_catalog`].
    pub item_shop_data: Option<crate::shop_catalog::ShopItemData>,
    /// Gold shops located in the active scene's MAN, priced from
    /// [`Self::item_shop_data`]. Repopulated on each field-scene entry by
    /// [`crate::scene::SceneHost::enter_field_scene`]; empty when the scene has
    /// no merchant or the disc isn't available. The field-menu shop-open path
    /// picks from these instead of a hand-authored stock list.
    pub scene_shops: Vec<crate::shop_catalog::SceneShop>,
    /// A priced shop the field VM has just opened (op `0x49` sub-0 inline shop
    /// record - see [`Self::try_arm_field_shop`]). The host drains it with
    /// [`Self::take_pending_field_shop`] to drive the buy/sell UI, then calls
    /// [`Self::finish_field_shop`] when the player leaves so the field VM
    /// resumes past the op. `None` between shop opens.
    pub pending_shop: Option<crate::shop::ShopSession>,
    /// `true` from the frame a field-VM shop op (`0x49` sub-0) is recognised
    /// until the op's resume runs - it gates the op-0x49 tristate so the VM
    /// stays suspended while the shop is up. Distinguishes a shop arm from the
    /// name-entry arm and a plain script yield.
    pub shop_armed: bool,
    /// `true` while the opened shop UI is still up; the host clears it via
    /// [`Self::finish_field_shop`] so the op-0x49 tristate flips Armed -> Done.
    pub shop_open: bool,
    /// The casino prize table (menu overlay PROT 899 file `0x15D00`, four
    /// `0x60`-byte blocks), installed by
    /// [`Self::install_menu_overlay_tables`]. Empty without disc data - the
    /// prize counter then refuses to open rather than selling from an
    /// invented list.
    pub prize_blocks: Vec<Vec<crate::prize_exchange::PrizeRecord>>,
    /// A prize-exchange session the field VM just opened (op `0x49` sub-op 7,
    /// the casino prize counter - see [`Self::try_arm_prize_exchange`]). The
    /// host drains it with [`Self::take_pending_prize_exchange`] into its
    /// menu runtime, then calls [`Self::finish_prize_exchange`] when the
    /// player leaves.
    pub pending_prize_exchange: Option<crate::prize_exchange::PrizeExchangeSession>,
    /// `true` from the frame an op-`0x49` sub-op-7 arm is recognised until
    /// the op's resume runs - gates the op-0x49 tristate like
    /// [`Self::field_shop_armed`] does for the gold shop.
    pub prize_exchange_armed: bool,
    /// `true` while the opened prize-exchange UI is still up; cleared via
    /// [`Self::finish_prize_exchange`] so the tristate flips Armed -> Done.
    pub prize_exchange_open: bool,
}

impl ShopState {
    pub fn new() -> Self {
        Self {
            item_shop_data: None,
            scene_shops: Vec::new(),
            pending_shop: None,
            shop_armed: false,
            prize_blocks: Vec::new(),
            pending_prize_exchange: None,
            prize_exchange_armed: false,
            prize_exchange_open: false,
            shop_open: false,
        }
    }
}

impl Default for ShopState {
    fn default() -> Self {
        Self::new()
    }
}
