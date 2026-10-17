//! Match variants as a typed set (versus.md §13). [`Variant`] lists all 89
//! switches in catalogue (= menu) order; [`VariantSet`] stores one bit per
//! player slot for per-player variants and bit 0 for global ones, and
//! applies the link groups (turning one on turns its partners off).
//!
//! Most variants are not implemented yet; the set is the single source of
//! truth so menus, the intro icon strip and the sim agree. The legacy
//! [`crate::world::Variants`] bool struct is derived from it per player.

use crate::checksum::{Hasher, StableHash};

macro_rules! variants {
    ($( $name:ident = $title:literal, $pp:literal ;)*) => {
        /// The variants in catalogue order.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum Variant { $( $name, )* }

        impl Variant {
            pub const ALL: &'static [Variant] = &[ $( Variant::$name, )* ];
            /// Menu title (field name split at capitals, upper-cased).
            pub fn title(self) -> &'static str {
                match self { $( Variant::$name => $title, )* }
            }
            /// Per-player (one flag per slot) or global.
            pub fn per_player(self) -> bool {
                match self { $( Variant::$name => $pp, )* }
            }
        }
    };
}

variants! {
    // RULES
    NoAutobalance = "NO AUTOBALANCE", false;
    WeakAutobalance = "WEAK AUTOBALANCE", false;
    NoFriendlyFire = "NO FRIENDLY FIRE", false;
    NoTimeLimit = "NO TIME LIMIT", false;
    SuddenDeath = "SUDDEN DEATH", false;
    TeamRevive = "TEAM REVIVE", false;
    GunnStyle = "GUNN STYLE", false;
    // TREASURE
    NoTreasure = "NO TREASURE", false;
    MaxTreasure = "MAX TREASURE", false;
    AlwaysBigTreasure = "ALWAYS BIG TREASURE", false;
    BottomlessTreasure = "BOTTOMLESS TREASURE", false;
    BombChests = "BOMB CHESTS", false;
    SymmetricalTreasure = "SYMMETRICAL TREASURE", false;
    ShowTreasureSpawns = "SHOW TREASURE SPAWNS", false;
    TreasureDraft = "TREASURE DRAFT", false;
    // ARROWS
    NoArrows = "NO ARROWS", true;
    SingleArrow = "SINGLE ARROW", true;
    MaxArrows = "MAX ARROWS", true;
    RegeneratingArrows = "REGENERATING ARROWS", true;
    SecretArrows = "SECRET ARROWS", true;
    StartWithBombArrows = "START WITH BOMB ARROWS", true;
    StartWithLaserArrows = "START WITH LASER ARROWS", true;
    StartWithBrambleArrows = "START WITH BRAMBLE ARROWS", true;
    StartWithDrillArrows = "START WITH DRILL ARROWS", true;
    StartWithBoltArrows = "START WITH BOLT ARROWS", true;
    StartWithSuperBombArrows = "START WITH SUPER BOMB ARROWS", true;
    StartWithFeatherArrows = "START WITH FEATHER ARROWS", true;
    StartWithTriggerArrows = "START WITH TRIGGER ARROWS", true;
    StartWithPrismArrows = "START WITH PRISM ARROWS", true;
    StartWithRandomArrows = "START WITH RANDOM ARROWS", true;
    StartWithToyArrows = "START WITH TOY ARROWS", true;
    InfiniteLasers = "INFINITE LASERS", true;
    InfiniteDrills = "INFINITE DRILLS", true;
    InfiniteBrambles = "INFINITE BRAMBLES", true;
    NoSeekingArrows = "NO SEEKING ARROWS", true;
    SuperSeekingArrows = "SUPER SEEKING ARROWS", true;
    AntiGravityArrows = "ANTI GRAVITY ARROWS", true;
    // ARCHERS
    StartWithShields = "START WITH SHIELDS", true;
    RegeneratingShields = "REGENERATING SHIELDS", true;
    StartWithWings = "START WITH WINGS", true;
    StartWithSpeedBoots = "START WITH SPEED BOOTS", true;
    StartInvisible = "START INVISIBLE", true;
    StealthArchers = "STEALTH ARCHERS", true;
    DoubleJumping = "DOUBLE JUMPING", true;
    ExplodingCorpses = "EXPLODING CORPSES", true;
    TriggerCorpses = "TRIGGER CORPSES", true;
    ReturnAsGhosts = "RETURN AS GHOSTS", true;
    CorpsesDropArrows = "CORPSES DROP ARROWS", true;
    Encumbrance = "ENCUMBRANCE", true;
    SmallQuivers = "SMALL QUIVERS", true;
    NoQuivers = "NO QUIVERS", true;
    NoDodging = "NO DODGING", true;
    SuperDodging = "SUPER DODGING", true;
    ClumsyArchers = "CLUMSY ARCHERS", true;
    ShowDodgeCooldown = "SHOW DODGE COOLDOWN", true;
    FreeAiming = "FREE AIMING", true;
    NoSlipping = "NO SLIPPING", true;
    SlipperyFloors = "SLIPPERY FLOORS", true;
    // CURSES
    CursedBows = "CURSED BOWS", true;
    CursedDodges = "CURSED DODGES", true;
    BigHeads = "BIG HEADS", true;
    // ARENA
    NoHazards = "NO HAZARDS", false;
    AlwaysDark = "ALWAYS DARK", false;
    SlowTime = "SLOW TIME", false;
    AlwaysLava = "ALWAYS LAVA", false;
    AlwaysScrolling = "ALWAYS SCROLLING", false;
    OffsetWorld = "OFFSET WORLD", false;
    DarkPortals = "DARK PORTALS", false;
    // POWERUPS
    IgnoreTowerItemSet = "IGNORE TOWER ITEM SET", false;
    ArrowShuffle = "ARROW SHUFFLE", false;
    NoShield = "NO SHIELD", false;
    NoWings = "NO WINGS", false;
    NoSpeedBoots = "NO SPEED BOOTS", false;
    NoLookingGlass = "NO LOOKING GLASS", false;
    NoExtraArrows = "NO EXTRA ARROWS", false;
    NoBombArrows = "NO BOMB ARROWS", false;
    NoLaserArrows = "NO LASER ARROWS", false;
    NoBrambleArrows = "NO BRAMBLE ARROWS", false;
    NoDrillArrows = "NO DRILL ARROWS", false;
    NoBoltArrows = "NO BOLT ARROWS", false;
    NoSuperBombArrows = "NO SUPER BOMB ARROWS", false;
    NoFeatherArrows = "NO FEATHER ARROWS", false;
    NoTriggerArrows = "NO TRIGGER ARROWS", false;
    NoPrismArrows = "NO PRISM ARROWS", false;
    NoBomb = "NO BOMB", false;
    NoDarkOrb = "NO DARK ORB", false;
    NoTimeOrb = "NO TIME ORB", false;
    NoLavaOrb = "NO LAVA ORB", false;
    NoSpaceOrb = "NO SPACE ORB", false;
}

pub const VARIANT_COUNT: usize = 89;

use Variant as V;

/// Link groups (versus.md §13.1): turning one member on turns every other
/// member of the same group off.
const LINKS: &[&[Variant]] = &[
    &[V::NoAutobalance, V::WeakAutobalance],
    &[V::NoTimeLimit, V::SuddenDeath],
    &[V::TeamRevive, V::ExplodingCorpses],
    &[
        V::NoTreasure,
        V::MaxTreasure,
        V::AlwaysBigTreasure,
        V::BottomlessTreasure,
        V::BombChests,
    ],
    &[V::NoTreasure, V::SymmetricalTreasure],
    &[V::NoTreasure, V::ShowTreasureSpawns],
    &[V::TreasureDraft, V::NoTreasure, V::MaxTreasure],
    &[V::SingleArrow, V::MaxArrows, V::NoArrows],
    &[
        V::StartWithBombArrows,
        V::StartWithLaserArrows,
        V::StartWithBrambleArrows,
        V::StartWithDrillArrows,
        V::StartWithBoltArrows,
        V::StartWithSuperBombArrows,
        V::StartWithFeatherArrows,
        V::StartWithRandomArrows,
        V::StartWithToyArrows,
        V::StartWithTriggerArrows,
        V::StartWithPrismArrows,
    ],
    &[V::NoSeekingArrows, V::SuperSeekingArrows],
    &[V::SecretArrows, V::StartInvisible],
    &[V::NoDodging, V::SuperDodging],
    &[V::NoDodging, V::ShowDodgeCooldown],
    &[V::NoDodging, V::CursedDodges],
    &[V::NoDodging, V::ClumsyArchers],
    &[V::NoSlipping, V::SlipperyFloors],
    &[V::SmallQuivers, V::NoQuivers],
    &[V::ExplodingCorpses, V::TriggerCorpses],
    &[V::TriggerCorpses, V::ReturnAsGhosts],
    &[V::AlwaysScrolling, V::OffsetWorld],
];

/// The variant switches of a match: one bit per player slot (per-player
/// variants) or bit 0 (global variants).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VariantSet {
    bits: [u16; VARIANT_COUNT],
}

impl Default for VariantSet {
    fn default() -> Self {
        VariantSet {
            bits: [0; VARIANT_COUNT],
        }
    }
}

impl StableHash for VariantSet {
    fn stable_hash(&self, h: &mut Hasher) {
        for b in self.bits {
            h.u32(b as u32);
        }
    }
}

impl VariantSet {
    /// Slot `p`'s flag (`p < 0` reads false; global variants ignore `p`).
    pub fn get(&self, v: Variant, p: i32) -> bool {
        let b = self.bits[v as usize];
        if !v.per_player() {
            return b != 0;
        }
        (0..16).contains(&p) && b & (1 << p) != 0
    }

    /// `Value`: on for any slot (or the global flag).
    pub fn any(&self, v: Variant) -> bool {
        self.bits[v as usize] != 0
    }

    /// `Value = on` for every joined slot (`joined` = slot bit mask).
    pub fn set_all(&mut self, v: Variant, on: bool, joined: u16) {
        let mask = if v.per_player() { joined } else { 1 };
        if on {
            for g in LINKS.iter().filter(|g| g.contains(&v)) {
                for &o in g.iter().filter(|&&o| o != v) {
                    self.bits[o as usize] = 0;
                }
            }
            self.bits[v as usize] = mask;
        } else {
            self.bits[v as usize] = 0;
        }
    }

    /// `v[p] = on` (per-player); links are cleared for that slot only.
    pub fn set(&mut self, v: Variant, p: usize, on: bool) {
        if !v.per_player() {
            self.set_all(v, on, 1);
            return;
        }
        let bit = 1u16 << p;
        if on {
            for g in LINKS.iter().filter(|g| g.contains(&v)) {
                for &o in g.iter().filter(|&&o| o != v) {
                    self.bits[o as usize] &= !bit;
                }
            }
            self.bits[v as usize] |= bit;
        } else {
            self.bits[v as usize] &= !bit;
        }
    }

    /// Clear flags of slots that are not joined.
    pub fn clean(&mut self, joined: u16) {
        for v in Variant::ALL {
            if v.per_player() {
                self.bits[*v as usize] &= joined;
            }
        }
    }

    /// Active variants in catalogue order.
    pub fn active(&self) -> impl Iterator<Item = Variant> + '_ {
        Variant::ALL.iter().copied().filter(|&v| self.any(v))
    }

    /// The legacy per-feature bools for slot `p`.
    pub fn legacy(&self, p: i32) -> crate::world::Variants {
        let g = |v| self.get(v, p);
        crate::world::Variants {
            free_aiming: g(V::FreeAiming),
            double_jumping: g(V::DoubleJumping),
            no_dodging: g(V::NoDodging),
            super_dodging: g(V::SuperDodging),
            cursed_bows: g(V::CursedBows),
            cursed_dodges: g(V::CursedDodges),
            slippery_floors: g(V::SlipperyFloors),
            no_slipping: g(V::NoSlipping),
            encumbrance: g(V::Encumbrance),
            regenerating_arrows: g(V::RegeneratingArrows),
            regenerating_shields: g(V::RegeneratingShields),
            anti_gravity_arrows: g(V::AntiGravityArrows),
            no_seeking_arrows: g(V::NoSeekingArrows),
            super_seeking_arrows: g(V::SuperSeekingArrows),
            no_friendly_fire: g(V::NoFriendlyFire),
            slow_time: g(V::SlowTime),
            clumsy_archers: g(V::ClumsyArchers),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_and_links() {
        assert_eq!(Variant::ALL.len(), VARIANT_COUNT);
        let mut s = VariantSet::default();
        s.set_all(V::NoTimeLimit, true, 0b11);
        s.set_all(V::SuddenDeath, true, 0b11);
        assert!(!s.any(V::NoTimeLimit) && s.any(V::SuddenDeath));
        s.set(V::SingleArrow, 1, true);
        assert!(s.get(V::SingleArrow, 1) && !s.get(V::SingleArrow, 0));
        s.set(V::MaxArrows, 1, true);
        assert!(!s.get(V::SingleArrow, 1));
        assert!(!s.get(V::MaxArrows, -1));
        assert_eq!(V::StartWithBombArrows.title(), "START WITH BOMB ARROWS");
    }
}
