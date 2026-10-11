//! The sub-screen id space and the table routing each id to the port that implements it.

/// Sub-screen ids, as indexed out of the retail pointer table.
///
/// The id space is the table's, so the discriminants are the retail
/// numbers and a transition can be written as the number the decompile
/// stores. Ids the table fills with screens whose behaviour is not yet
/// pinned are [`SaveSubScreen::Unpinned`], which keeps the space total.
///
/// PORT: FUN_801E4F40 (the table this indexes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSubScreen {
    /// `0x00` - terminal screen; exits the save flow with code 3.
    FinalExit,
    /// `0x01` - the slot selector.
    SlotSelect,
    /// `0x02` - the developer **character-parameter editor**
    /// (`FUN_801D6E18`), not a save screen. Its twelve cursor rows write
    /// the live character record directly: XP `+0x000`, HP / MP max
    /// `+0x11C` / `+0x11E`, the six record stats `+0x122..+0x12C`, the
    /// magic rank `+0x130` and the skill roster `+0x185` / `+0x186`. The
    /// name this variant used to carry ("save entry, reached from the
    /// pause menu") is contradicted by those stores.
    CharParamEditor,
    /// `0x03` - Yes/No confirm, cursor defaulting to `No`.
    ConfirmYesNo,
    /// `0x04` - post-save "press any button" return.
    PostSaveReturn,
    /// `0x08` - message screen that waits for the pad to be *released*.
    PadReleaseWait,
    /// `0x0B` - Yes/No confirm whose Yes branch exits with code 4.
    ConfirmExit,
    /// `0x12` - scrollable party-count picker.
    PartyPicker,
    /// `0x17` - generic picker wrapper over slots `0..=9`.
    GenericPicker,
    /// `0x18` - load-card driver (card to RAM), `FUN_801DAE24`.
    ///
    /// The pair's direction is fixed by the root picker's row labels: row
    /// `5` is `@Load` and routes here, row `6` is `@Save` and routes to
    /// [`Self::CardSave`] - see
    /// `crate::pause_screens::ROOT_MENU_ROUTES`. Retail's own op selector
    /// agrees: this driver calls `FUN_801DD35C(1, 2)`, the arm that skips
    /// the card-file erase.
    CardLoad,
    /// `0x19` - save-card driver (RAM to card), `FUN_801DAEF4`.
    ///
    /// Calls `FUN_801DD35C(1, 1)`, the arm that looks the file up and
    /// erases it before writing (`FUN_801E37CC`, ported as
    /// [`crate::card_bu_io::erase_file`]).
    CardSave,
    /// `0x1A` - the shop's **Buy / Sell / Quit mode select**
    /// (`FUN_801DAFD4`), not a save-slot confirm.
    ///
    /// The two halves that kept the old name alive are settled together.
    /// `save-screen.md` reads the screen's row-`1` validation as a walk of
    /// the item bag at `0x80085958` ("own anything to sell"), not a
    /// save-block existence table - and `0x1B`..`0x1F` around it are all
    /// shop screens. The entry-context decode agrees from the other side:
    /// context byte `0x00` routes here (`0x801DC89C..0x801DC8A0`), and
    /// that byte is the shop's own record kind, which is why the fallback
    /// below is now [`SaveEntryContext::ShopEntry`].
    ShopModeSelect,
    /// `0x1E` - inventory spinner ahead of the quantity screen. In context
    /// (`save-screen.md`) this is the shop **sell list**; the staged
    /// "inventory bytes" are the bag slot the sell-quantity screen consumes.
    QuantitySpinner,
    /// `0x20` - the casino **prize-exchange** confirm (`FUN_801DC1CC`),
    /// not an auto-save. It reads the block byte from the entry-context
    /// record `_DAT_8007B450[1]` and indexes the prize table at
    /// `0x801E4518 + block*0x60`, which is the casino table and nothing
    /// else. `docs/subsystems/field-menu.md` pins the same id to window
    /// 46 from the widget-script sweep, independently.
    CasinoPrizeConfirm,
    /// `0x16` - a bare frame-flush tick. `FUN_801DD310` is eight
    /// instructions - prologue, `jal 0x80031D00`, epilogue - so the slot
    /// exists to keep the table dense and does nothing else; the flush it
    /// tail-calls is the frame-end / actor-tick one every dispatch pass
    /// already runs. Pinned, not unknown: there is nothing further to learn
    /// from the bytes.
    FrameFlushTick,
    /// A table slot whose screen **another `engine-core` module ports**.
    /// Carries its retail id; [`SaveSubScreen::routed_port`] names the
    /// routine and the module.
    ///
    /// This is not a softer spelling of [`Self::Unpinned`]. Every id here
    /// has a `// PORT:` tag on live engine code reached from a host - the
    /// pause-menu Items flow, the Magic flow, the Equip flow, the shop, and
    /// this module's own `0x15` list helpers. What the id lacks is a step
    /// machine *in this module*, which is a statement about where the port
    /// lives rather than about whether the screen is understood.
    Routed(u8),
    /// A table slot whose screen is not yet pinned. Carries its id so a
    /// transition into one round-trips.
    ///
    /// After the [`Self::Routed`] split this covers **no live table slot**:
    /// the retail table is `0x00..=0x20` and every one of those 33 ids is
    /// either pinned above or routed. It remains so the id space stays
    /// total for an out-of-range byte, which retail itself reads as `0`
    /// past the end of the table.
    Unpinned(u8),
}

/// Where a [`SaveSubScreen::Routed`] id's port lives.
///
/// The `engine_module` / `engine_item` pair is the chain a reader follows to
/// the code that runs the screen; `retail_fn` is the entry the sub-screen
/// pointer table holds for the id. All three are `&'static str` rather than
/// typed handles on purpose - this is a map from one id space to another, and
/// the modules it names have no common session trait to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutedPort {
    /// The retail table entry for the id.
    pub retail_fn: &'static str,
    /// The `engine-core` module that ports it.
    pub engine_module: &'static str,
    /// The item in that module a host enters the screen through.
    pub engine_item: &'static str,
}

/// The routed ids, in table order: `(id, retail fn, module, item)`.
///
/// Each row's `retail_fn` is the entry `docs/subsystems/save-screen.md`'s
/// pointer-table section lists for the id, and each `engine_module` carries a
/// `// PORT:` tag naming that same routine - so a row that drifts is
/// detectable by grepping for the address, which
/// [`tests::every_routed_id_names_a_distinct_port`] does structurally.
pub(super) const ROUTED_PORTS: &[(u8, RoutedPort)] = &[
    (
        0x05,
        RoutedPort {
            retail_fn: "FUN_801D7C00",
            engine_module: "crate::pause_screens",
            engine_item: "PauseItemsSession (the Use / Throw Out / Arrange command window)",
        },
    ),
    (
        0x06,
        RoutedPort {
            retail_fn: "FUN_801D7E50",
            engine_module: "crate::pause_screens",
            engine_item: "PauseItemsSession (the Use list + effect-class dispatch)",
        },
    ),
    (
        0x07,
        RoutedPort {
            retail_fn: "FUN_801D8734",
            engine_module: "crate::pause_screens",
            engine_item: "PauseItemsSession (the Throw Out list + confirm)",
        },
    ),
    (
        0x09,
        RoutedPort {
            retail_fn: "FUN_801D7FF8",
            engine_module: "crate::pause_screens",
            engine_item: "PauseItemsSession (the ApplyAll route)",
        },
    ),
    (
        0x0A,
        RoutedPort {
            retail_fn: "FUN_801D8308",
            engine_module: "crate::pause_screens",
            engine_item: "PauseItemsSession (the ApplySingle route)",
        },
    ),
    (
        0x0C,
        RoutedPort {
            retail_fn: "FUN_801D8B90",
            engine_module: "crate::pause_screens",
            engine_item: "SpecialUseSession (the Door of Wind destination list)",
        },
    ),
    (
        0x0D,
        RoutedPort {
            retail_fn: "FUN_801D8D94",
            engine_module: "crate::pause_screens",
            engine_item: "SpecialUseSession (the Incense confirm + class-0x82 apply)",
        },
    ),
    (
        0x0E,
        RoutedPort {
            retail_fn: "FUN_801D8F10",
            engine_module: "crate::spell_menu",
            engine_item: "SpellMenuSession (the CharSelect phase)",
        },
    ),
    (
        0x0F,
        RoutedPort {
            retail_fn: "FUN_801D9110",
            engine_module: "crate::spell_menu",
            engine_item: "SpellMenuSession (the SpellSelect phase + spell_targets_group)",
        },
    ),
    (
        0x10,
        RoutedPort {
            retail_fn: "FUN_801D9280",
            engine_module: "crate::spell_menu",
            engine_item: "SpellMenuSession (the group-cast confirm)",
        },
    ),
    (
        0x11,
        RoutedPort {
            retail_fn: "FUN_801D9594",
            engine_module: "crate::spell_menu",
            engine_item: "SpellMenuSession (the TargetSelect phase + resolve)",
        },
    ),
    (
        0x13,
        RoutedPort {
            retail_fn: "FUN_801D99F0",
            engine_module: "crate::equip_session",
            engine_item: "EquipSession (the slot browse + Best Equipment row)",
        },
    ),
    (
        0x14,
        RoutedPort {
            retail_fn: "FUN_801D9C14",
            engine_module: "crate::equip_session",
            engine_item: "EquipSession (the candidate list + trial-equip commit)",
        },
    ),
    (
        0x15,
        RoutedPort {
            retail_fn: "FUN_801DA2A0",
            engine_module: "crate::save_subscreen",
            engine_item: "sub15_list_source / sub15_list_len / sub15_list_frame",
        },
    ),
    (
        0x1B,
        RoutedPort {
            retail_fn: "FUN_801DB21C",
            engine_module: "crate::shop",
            engine_item: "shop::buy_list_confirm_route",
        },
    ),
    (
        0x1C,
        RoutedPort {
            retail_fn: "FUN_801DB380",
            engine_module: "crate::shop",
            engine_item: "shop (the buy recipient picker)",
        },
    ),
    (
        0x1D,
        RoutedPort {
            retail_fn: "FUN_801DB7F4",
            engine_module: "crate::shop",
            engine_item: "shop::BuyQuantitySession",
        },
    ),
    (
        0x1F,
        RoutedPort {
            retail_fn: "FUN_801DBD94",
            engine_module: "crate::shop",
            engine_item: "shop (the sell-quantity input + commit)",
        },
    ),
];

/// The [`RoutedPort`] for a retail sub-screen id, if another module ports it.
pub fn routed_port(id: u8) -> Option<RoutedPort> {
    ROUTED_PORTS
        .iter()
        .find_map(|(k, port)| (*k == id).then_some(*port))
}

impl SaveSubScreen {
    /// The retail table index for this screen.
    pub fn id(self) -> u8 {
        match self {
            Self::FinalExit => 0x00,
            Self::SlotSelect => 0x01,
            Self::CharParamEditor => 0x02,
            Self::ConfirmYesNo => 0x03,
            Self::PostSaveReturn => 0x04,
            Self::PadReleaseWait => 0x08,
            Self::ConfirmExit => 0x0B,
            Self::PartyPicker => 0x12,
            Self::GenericPicker => 0x17,
            Self::CardLoad => 0x18,
            Self::CardSave => 0x19,
            Self::ShopModeSelect => 0x1A,
            Self::QuantitySpinner => 0x1E,
            Self::CasinoPrizeConfirm => 0x20,
            Self::FrameFlushTick => 0x16,
            Self::Routed(id) | Self::Unpinned(id) => id,
        }
    }

    /// Resolve a retail table index into a screen.
    pub fn from_id(id: u8) -> Self {
        match id {
            0x00 => Self::FinalExit,
            0x01 => Self::SlotSelect,
            0x02 => Self::CharParamEditor,
            0x03 => Self::ConfirmYesNo,
            0x04 => Self::PostSaveReturn,
            0x08 => Self::PadReleaseWait,
            0x0B => Self::ConfirmExit,
            0x12 => Self::PartyPicker,
            0x17 => Self::GenericPicker,
            0x18 => Self::CardLoad,
            0x19 => Self::CardSave,
            0x1A => Self::ShopModeSelect,
            0x1E => Self::QuantitySpinner,
            0x20 => Self::CasinoPrizeConfirm,
            0x16 => Self::FrameFlushTick,
            other if routed_port(other).is_some() => Self::Routed(other),
            other => Self::Unpinned(other),
        }
    }

    /// Whether this module carries a step machine for the screen.
    ///
    /// A [`Self::Routed`] id is **not** pinned by this predicate and that is
    /// deliberate: the question it answers is "does [`SaveScreenMachine`]
    /// step it", and for a routed id the answer is no - the owning module
    /// does. Use [`Self::routed_port`] to ask the other question.
    pub fn is_pinned(self) -> bool {
        !matches!(self, Self::Unpinned(_) | Self::Routed(_))
    }

    /// Where this screen's port lives, for an id another module owns.
    pub fn routed_port(self) -> Option<RoutedPort> {
        match self {
            Self::Routed(id) => routed_port(id),
            _ => None,
        }
    }
}
