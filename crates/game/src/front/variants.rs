//! Match variants as the menus see them (versus.md 13, ui.md 8): the
//! catalogue in menu order with its attributes, and the per-slot values
//! with the original's link and per-player rules.

pub const SLOTS: usize = 12;

/// Menu sections, in order.
pub const SECTIONS: [&str; 7] = [
    "RULES", "TREASURE", "ARROWS", "ARCHERS", "CURSES", "ARENA", "POWERUPS",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unlock {
    None,
    GunnStyle,
    SunkenCity,
    TowerForge,
    Ascension,
    PurpleArcher,
}

/// One variant's static attributes.
#[derive(Clone, Copy, Debug)]
pub struct VariantDef {
    /// Field name (`NoAutobalance`); title and icon derive from it.
    pub field: &'static str,
    pub section: usize,
    pub per_player: bool,
    pub can_random: bool,
    pub dark_world: bool,
    pub unlock: Unlock,
    pub tournament: bool,
    pub team_tournament: bool,
    /// Not in the round-intro icon strip (item exclusions).
    pub unlisted: bool,
    pub scroll_effect: bool,
    pub description: &'static str,
}

const fn v(field: &'static str, section: usize) -> VariantDef {
    VariantDef {
        field,
        section,
        per_player: false,
        can_random: false,
        dark_world: false,
        unlock: Unlock::None,
        tournament: false,
        team_tournament: false,
        unlisted: false,
        scroll_effect: false,
        description: "",
    }
}

const fn pp(mut d: VariantDef) -> VariantDef {
    d.per_player = true;
    d
}
const fn r(mut d: VariantDef) -> VariantDef {
    d.can_random = true;
    d
}
const fn dw(mut d: VariantDef) -> VariantDef {
    d.dark_world = true;
    d
}
const fn lock(mut d: VariantDef, u: Unlock) -> VariantDef {
    d.unlock = u;
    d
}
const fn desc(mut d: VariantDef, s: &'static str) -> VariantDef {
    d.description = s;
    d
}
const fn un(mut d: VariantDef) -> VariantDef {
    d.unlisted = true;
    d
}

/// The catalogue (Steam, 89 entries) in declaration = menu order.
pub const VARIANTS: &[VariantDef] = &{
    use Unlock::*;
    let mut t1 = v("NoAutobalance", 0);
    t1.tournament = true;
    t1.team_tournament = true;
    t1.description = "DISABLES SCORE-BASED ARROW AND SHIELD ADVANTAGES";
    let mut team_revive = dw(desc(
        v("TeamRevive", 0),
        "IN TEAM DEATHMATCH MODE, RESURRECT YOUR DEAD FRIENDS",
    ));
    team_revive.team_tournament = true;
    let mut sym = r(v("SymmetricalTreasure", 1));
    sym.tournament = true;
    sym.team_tournament = true;
    let mut always_scrolling = r(v("AlwaysScrolling", 5));
    always_scrolling.scroll_effect = true;
    let mut offset_world = desc(v("OffsetWorld", 5), "JUST TRY IT");
    offset_world.scroll_effect = true;
    let mut no_space_orb = un(v("NoSpaceOrb", 6));
    no_space_orb.scroll_effect = true;
    [
        // RULES
        t1,
        desc(
            v("WeakAutobalance", 0),
            "DISABLES SCORE-BASED SHIELD ADVANTAGES",
        ),
        desc(v("NoFriendlyFire", 0), "PREVENTS HURTING SELF OR TEAMMATES"),
        v("NoTimeLimit", 0),
        r(v("SuddenDeath", 0)),
        team_revive,
        r(lock(v("GunnStyle", 0), GunnStyle)),
        // TREASURE
        r(v("NoTreasure", 1)),
        r(v("MaxTreasure", 1)),
        r(v("AlwaysBigTreasure", 1)),
        r(v("BottomlessTreasure", 1)),
        r(v("BombChests", 1)),
        sym,
        r(v("ShowTreasureSpawns", 1)),
        r(dw(desc(
            v("TreasureDraft", 1),
            "CHOOSE YOUR TREASURE AT THE START OF EACH ROUND",
        ))),
        // ARROWS
        pp(v("NoArrows", 2)),
        pp(r(v("SingleArrow", 2))),
        pp(r(v("MaxArrows", 2))),
        pp(r(desc(
            v("RegeneratingArrows", 2),
            "IF YOU HAVE NO ARROWS, YOU WILL GAIN ONE",
        ))),
        pp(r(desc(v("SecretArrows", 2), "HIDES ARROW INVENTORIES"))),
        pp(r(v("StartWithBombArrows", 2))),
        pp(r(v("StartWithLaserArrows", 2))),
        pp(r(v("StartWithBrambleArrows", 2))),
        pp(v("StartWithDrillArrows", 2)),
        pp(r(lock(v("StartWithBoltArrows", 2), SunkenCity))),
        pp(lock(v("StartWithSuperBombArrows", 2), TowerForge)),
        pp(r(lock(v("StartWithFeatherArrows", 2), Ascension))),
        pp(r(dw(v("StartWithTriggerArrows", 2)))),
        pp(r(dw(v("StartWithPrismArrows", 2)))),
        pp(r(v("StartWithRandomArrows", 2))),
        pp(r(v("StartWithToyArrows", 2))),
        pp(r(desc(v("InfiniteLasers", 2), "LASERS BOUNCE FOREVER"))),
        pp(r(desc(v("InfiniteDrills", 2), "DRILLS DRILL FOREVER"))),
        pp(r(dw(desc(
            v("InfiniteBrambles", 2),
            "BRAMBLES STAY FOREVER",
        )))),
        pp(r(v("NoSeekingArrows", 2))),
        pp(r(v("SuperSeekingArrows", 2))),
        pp(r(v("AntiGravityArrows", 2))),
        // ARCHERS
        pp(r(v("StartWithShields", 3))),
        pp(r(v("RegeneratingShields", 3))),
        pp(r(v("StartWithWings", 3))),
        pp(r(v("StartWithSpeedBoots", 3))),
        pp(r(v("StartInvisible", 3))),
        pp(r(desc(
            v("StealthArchers", 3),
            "ARCHERS DO NOT GIVE OFF LIGHT",
        ))),
        pp(r(dw(desc(v("DoubleJumping", 3), "JUMP AGAIN IN THE AIR")))),
        pp(r(v("ExplodingCorpses", 3))),
        pp(r(desc(
            v("TriggerCorpses", 3),
            "HOLD SHOOT BUTTON TO EXPLODE YOUR CORPSE!",
        ))),
        pp(r(desc(
            v("ReturnAsGhosts", 3),
            "SEEK REVENGE FROM BEYOND THE GRAVE",
        ))),
        pp(r(v("CorpsesDropArrows", 3))),
        pp(r(desc(
            v("Encumbrance", 3),
            "ARROWS IN INVENTORY REDUCE MOVEMENT SPEED",
        ))),
        pp(r(desc(
            v("SmallQuivers", 3),
            "ARCHERS CAN ONLY HOLD THREE ARROWS",
        ))),
        pp(r(dw(desc(
            v("NoQuivers", 3),
            "ARCHERS CAN ONLY HOLD A SINGLE ARROW",
        )))),
        pp(r(v("NoDodging", 3))),
        pp(r(v("SuperDodging", 3))),
        pp(r(dw(desc(
            v("ClumsyArchers", 3),
            "DROP AN ARROW WHENEVER YOU DODGE",
        )))),
        pp(r(v("ShowDodgeCooldown", 3))),
        pp(r(v("FreeAiming", 3))),
        pp(v("NoSlipping", 3)),
        pp(r(v("SlipperyFloors", 3))),
        // CURSES
        pp(desc(
            v("CursedBows", 4),
            "SHOOTING WITH AN EMPTY QUIVER KILLS",
        )),
        pp(desc(
            v("CursedDodges", 4),
            "DODGING WITHOUT CATCHING AN ARROW OR STOMPING KILLS",
        )),
        pp(r(v("BigHeads", 4))),
        // ARENA
        r(dw(v("NoHazards", 5))),
        r(v("AlwaysDark", 5)),
        r(v("SlowTime", 5)),
        r(v("AlwaysLava", 5)),
        always_scrolling,
        offset_world,
        r(lock(v("DarkPortals", 5), PurpleArcher)),
        // POWERUPS
        desc(
            v("IgnoreTowerItemSet", 6),
            "ALLOWS ALL POWERUPS TO SPAWN ON ALL TOWERS",
        ),
        dw(desc(
            v("ArrowShuffle", 6),
            "PICKS TWO RANDOM ARROW TYPES TO INCLUDE",
        )),
        un(v("NoShield", 6)),
        un(v("NoWings", 6)),
        un(v("NoSpeedBoots", 6)),
        un(v("NoLookingGlass", 6)),
        un(v("NoExtraArrows", 6)),
        un(v("NoBombArrows", 6)),
        un(v("NoLaserArrows", 6)),
        un(v("NoBrambleArrows", 6)),
        un(v("NoDrillArrows", 6)),
        un(lock(v("NoBoltArrows", 6), SunkenCity)),
        un(lock(v("NoSuperBombArrows", 6), TowerForge)),
        un(lock(v("NoFeatherArrows", 6), Ascension)),
        un(dw(v("NoTriggerArrows", 6))),
        un(dw(v("NoPrismArrows", 6))),
        un(v("NoBomb", 6)),
        un(v("NoDarkOrb", 6)),
        un(v("NoTimeOrb", 6)),
        un(v("NoLavaOrb", 6)),
        no_space_orb,
    ]
};

/// Link groups (versus.md 13.1): within a group, turning one on turns the
/// others off.
const LINKS: &[&[&str]] = &[
    &["NoAutobalance", "WeakAutobalance"],
    &["NoTimeLimit", "SuddenDeath"],
    &["TeamRevive", "ExplodingCorpses"],
    &[
        "NoTreasure",
        "MaxTreasure",
        "AlwaysBigTreasure",
        "BottomlessTreasure",
        "BombChests",
    ],
    &["NoTreasure", "SymmetricalTreasure"],
    &["NoTreasure", "ShowTreasureSpawns"],
    &["TreasureDraft", "NoTreasure", "MaxTreasure"],
    &["SingleArrow", "MaxArrows", "NoArrows"],
    &[
        "StartWithBombArrows",
        "StartWithLaserArrows",
        "StartWithBrambleArrows",
        "StartWithDrillArrows",
        "StartWithBoltArrows",
        "StartWithSuperBombArrows",
        "StartWithFeatherArrows",
        "StartWithRandomArrows",
        "StartWithToyArrows",
        "StartWithTriggerArrows",
        "StartWithPrismArrows",
    ],
    &["NoSeekingArrows", "SuperSeekingArrows"],
    &["SecretArrows", "StartInvisible"],
    &["NoDodging", "SuperDodging"],
    &["NoDodging", "ShowDodgeCooldown"],
    &["NoDodging", "CursedDodges"],
    &["NoDodging", "ClumsyArchers"],
    &["NoSlipping", "SlipperyFloors"],
    &["SmallQuivers", "NoQuivers"],
    &["ExplodingCorpses", "TriggerCorpses"],
    &["TriggerCorpses", "ReturnAsGhosts"],
    &["AlwaysScrolling", "OffsetWorld"],
];

impl VariantDef {
    /// "NoAutobalance" -> "NO AUTOBALANCE".
    pub fn title(&self) -> String {
        let mut s = String::new();
        for (i, c) in self.field.chars().enumerate() {
            if i > 0 && c.is_ascii_uppercase() {
                s.push(' ');
            }
            s.push(c.to_ascii_uppercase());
        }
        s
    }

    /// Menu-atlas icon name.
    pub fn icon(&self) -> String {
        let mut c = self.field.chars();
        let first = c.next().map(|f| f.to_ascii_lowercase()).unwrap_or(' ');
        format!("variants/{first}{}", c.as_str())
    }
}

pub fn index_of(field: &str) -> Option<usize> {
    VARIANTS.iter().position(|v| v.field == field)
}

/// Indices linked to `i` (both directions, all groups containing it).
pub fn linked(i: usize) -> Vec<usize> {
    let f = VARIANTS[i].field;
    let mut out = Vec::new();
    for g in LINKS {
        if g.contains(&f) {
            for o in g.iter().filter(|o| **o != f) {
                if let Some(j) = index_of(o)
                    && !out.contains(&j)
                {
                    out.push(j);
                }
            }
        }
    }
    out
}

/// What the player has: a per-slot bitmask per variant (global variants
/// use bit 0 only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantSet {
    pub bits: Vec<u16>,
}

impl Default for VariantSet {
    fn default() -> Self {
        Self {
            bits: vec![0; VARIANTS.len()],
        }
    }
}

/// What the variant menu can show/use.
#[derive(Clone, Copy, Debug)]
pub struct Availability {
    pub dark_world: bool,
    pub scroll_effects: bool,
}

impl Availability {
    pub fn visible(&self, d: &VariantDef) -> bool {
        // The port: every unlock is granted when its content exists.
        !(d.scroll_effect && !self.scroll_effects)
    }

    pub fn usable(&self, d: &VariantDef) -> bool {
        self.visible(d) && (!d.dark_world || self.dark_world)
    }
}

impl VariantSet {
    fn joined_mask(joined: &[bool]) -> u16 {
        joined
            .iter()
            .enumerate()
            .filter(|(_, j)| **j)
            .fold(0, |m, (i, _)| m | 1 << i)
    }

    /// `Value`: on for anyone.
    pub fn value(&self, i: usize) -> bool {
        self.bits[i] != 0
    }

    pub fn slot(&self, i: usize, slot: usize) -> bool {
        if VARIANTS[i].per_player {
            self.bits[i] & 1 << slot != 0
        } else {
            self.bits[i] != 0
        }
    }

    /// `Value = on` (everyone joined); turning on clears the links.
    pub fn set(&mut self, i: usize, on: bool, joined: &[bool]) {
        let m = if VARIANTS[i].per_player {
            Self::joined_mask(joined)
        } else {
            1
        };
        if on {
            self.bits[i] = m;
            for j in linked(i) {
                self.bits[j] = 0;
            }
        } else {
            self.bits[i] = 0;
        }
    }

    /// `v[slot] = on` for a per-player variant (clears links on that slot).
    pub fn set_slot(&mut self, i: usize, slot: usize, on: bool) {
        if !VARIANTS[i].per_player {
            self.bits[i] = on as u16;
            return;
        }
        if on {
            self.bits[i] |= 1 << slot;
            for j in linked(i) {
                self.bits[j] &= !(1 << slot);
            }
        } else {
            self.bits[i] &= !(1 << slot);
        }
    }

    pub fn clear_all(&mut self) {
        self.bits.iter_mut().for_each(|b| *b = 0);
    }

    pub fn active(&self) -> impl Iterator<Item = usize> + '_ {
        (0..VARIANTS.len()).filter(|&i| self.value(i))
    }

    #[allow(dead_code)]
    pub fn active_count(&self) -> usize {
        self.active().count()
    }

    /// `Clean(old)`: per-player variants that were on for everybody of the
    /// old roster go to everybody of the new one; unjoined slots cleared.
    pub fn clean(&mut self, old_count: usize, joined: &[bool]) {
        let m = Self::joined_mask(joined);
        for (i, d) in VARIANTS.iter().enumerate() {
            if !d.per_player || self.bits[i] == 0 {
                continue;
            }
            if self.bits[i].count_ones() as usize == old_count {
                self.bits[i] = m;
            } else {
                self.bits[i] &= m;
            }
        }
    }

    /// Exactly the 1v1 (`team` = false) or 2v2 tournament set.
    pub fn apply_tournament(&mut self, team: bool, joined: &[bool]) {
        self.clear_all();
        for (i, d) in VARIANTS.iter().enumerate() {
            if (team && d.team_tournament) || (!team && d.tournament) {
                self.set(i, true, joined);
            }
        }
    }

    /// Every active variant carries the flag, and at least one is active.
    pub fn is_tournament(&self, team: bool) -> bool {
        let mut any = false;
        for i in self.active() {
            let d = &VARIANTS[i];
            if !(if team {
                d.team_tournament
            } else {
                d.tournament
            }) {
                return false;
            }
            any = true;
        }
        any
    }

    /// RANDOMIZE: clear every CanRandom variant, then pick 3-5 compatible
    /// ones in a shuffled order (cosmetic randomness, not the sim's).
    pub fn randomize(&mut self, avail: Availability, joined: &[bool], rng: &mut u64) {
        let mut pool: Vec<usize> = (0..VARIANTS.len())
            .filter(|&i| VARIANTS[i].can_random)
            .collect();
        for &i in &pool {
            self.bits[i] = 0;
        }
        for k in (1..pool.len()).rev() {
            let j = (next_rand(rng) % (k as u64 + 1)) as usize;
            pool.swap(k, j);
        }
        let want = 3 + (next_rand(rng) % 3) as usize;
        let mut chosen: Vec<usize> = Vec::new();
        for i in pool {
            if chosen.len() >= want {
                break;
            }
            if !avail.usable(&VARIANTS[i]) || chosen.iter().any(|&c| linked(c).contains(&i)) {
                continue;
            }
            chosen.push(i);
        }
        for i in chosen {
            self.set(i, true, joined);
        }
    }

    /// Preset storage: the titles of the active variants.
    pub fn to_titles(&self) -> Vec<String> {
        self.active().map(|i| VARIANTS[i].title()).collect()
    }

    pub fn load_titles(&mut self, titles: &[String], joined: &[bool]) {
        self.clear_all();
        for (i, d) in VARIANTS.iter().enumerate() {
            if titles.contains(&d.title()) {
                self.set(i, true, joined);
            }
        }
    }

    #[allow(dead_code)]
    pub fn by_field(&self, field: &str) -> bool {
        index_of(field).is_some_and(|i| self.value(i))
    }
}

/// xorshift64* for cosmetic menu randomness.
pub fn next_rand(state: &mut u64) -> u64 {
    let mut x = *state | 1;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue() {
        assert_eq!(VARIANTS.len(), 89);
        assert_eq!(VARIANTS[0].title(), "NO AUTOBALANCE");
        assert_eq!(VARIANTS[0].icon(), "variants/noAutobalance");
        let i = index_of("StartWithBombArrows").unwrap();
        assert_eq!(VARIANTS[i].title(), "START WITH BOMB ARROWS");
        assert_eq!(linked(i).len(), 10);
        let nd = index_of("NoDodging").unwrap();
        assert_eq!(linked(nd).len(), 4);
        // Partners of NoDodging are not linked to each other.
        assert!(
            !linked(index_of("SuperDodging").unwrap())
                .contains(&index_of("ClumsyArchers").unwrap())
        );
    }

    #[test]
    fn links_and_per_player() {
        let joined = [
            true, true, false, true, false, false, false, false, false, false, false, false,
        ];
        let mut s = VariantSet::default();
        let max = index_of("MaxArrows").unwrap();
        let single = index_of("SingleArrow").unwrap();
        s.set(max, true, &joined);
        assert_eq!(s.bits[max], 0b1011);
        s.set_slot(single, 1, true);
        assert!(s.slot(single, 1) && !s.slot(max, 1) && s.slot(max, 0));
        // Global links.
        let nt = index_of("NoTreasure").unwrap();
        let sym = index_of("SymmetricalTreasure").unwrap();
        s.set(sym, true, &joined);
        s.set(nt, true, &joined);
        assert!(!s.value(sym) && s.value(nt));
        // Tournament presets.
        s.apply_tournament(false, &joined);
        assert!(s.is_tournament(false));
        assert_eq!(s.active_count(), 2);
        assert!(
            s.is_tournament(true),
            "the 1v1 set is a subset of the 2v2 flags"
        );
        s.apply_tournament(true, &joined);
        assert_eq!(s.active_count(), 3);
        assert!(!s.is_tournament(false));
        // Clean: everyone -> everyone of the new roster.
        let mut s = VariantSet::default();
        s.set(max, true, &joined);
        let mut j2 = joined;
        j2[4] = true;
        s.clean(3, &j2);
        assert_eq!(s.bits[max], 0b11011);
        // Presets by title.
        let t = s.to_titles();
        let mut s2 = VariantSet::default();
        s2.load_titles(&t, &j2);
        assert_eq!(s2, s);
    }

    #[test]
    fn randomize_respects_links() {
        let joined = [true; 12];
        let avail = Availability {
            dark_world: false,
            scroll_effects: true,
        };
        let mut rng = 12345;
        for _ in 0..200 {
            let mut s = VariantSet::default();
            s.randomize(avail, &joined, &mut rng);
            let act: Vec<usize> = s.active().collect();
            assert!((3..=5).contains(&act.len()), "{act:?}");
            for &a in &act {
                assert!(!VARIANTS[a].dark_world);
                for &b in &act {
                    assert!(!linked(a).contains(&b));
                }
            }
        }
    }
}
