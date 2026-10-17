//! The quiver (`ArrowList`, player.md §3.4 / arrows.md §8): ordered arrow
//! types, index 0 fires next, kept sorted so the type with the highest
//! sort-set index comes first.

use crate::checksum::{Hasher, StableHash};
use crate::impl_hash_enum;

/// Arrow types in enum order (arrows.md §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum ArrowType {
    #[default]
    Normal = 0,
    Bomb,
    SuperBomb,
    Laser,
    Bramble,
    Drill,
    Bolt,
    Toy,
    Feather,
    Trigger,
    Prism,
}
impl_hash_enum!(ArrowType);

impl ArrowType {
    /// Low-priority types (Normal, Toy) are the first to be dropped.
    pub fn low_priority(self) -> bool {
        matches!(self, ArrowType::Normal | ArrowType::Toy)
    }
}

const CAP: usize = 16;
const TYPES: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quiver {
    arrows: [ArrowType; CAP],
    len: u8,
    sort_set: [ArrowType; TYPES],
    sort_len: u8,
    pub max: u8,
}

impl StableHash for Quiver {
    fn stable_hash(&self, h: &mut Hasher) {
        self.as_slice().stable_hash(h);
        self.sort_set[..self.sort_len as usize].stable_hash(h);
        h.u8(self.max);
    }
}

impl Default for Quiver {
    fn default() -> Self {
        Quiver {
            arrows: [ArrowType::Normal; CAP],
            len: 0,
            sort_set: [ArrowType::Normal; TYPES],
            sort_len: 0,
            max: 6,
        }
    }
}

impl Quiver {
    /// `ArrowList(n, type)`.
    pub fn new(n: usize, t: ArrowType) -> Quiver {
        let mut q = Quiver::default();
        let list = vec![t; n.min(CAP)];
        if n > 0 {
            q.add_arrows(&list);
        }
        q
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn as_slice(&self) -> &[ArrowType] {
        &self.arrows[..self.len as usize]
    }
    pub fn first(&self) -> Option<ArrowType> {
        self.as_slice().first().copied()
    }

    fn sort_index(&self, t: ArrowType) -> i32 {
        self.sort_set[..self.sort_len as usize]
            .iter()
            .position(|&s| s == t)
            .map(|i| i as i32)
            .unwrap_or(-1)
    }

    fn add_sort_type(&mut self, t: ArrowType) {
        if self.sort_index(t) >= 0 {
            return;
        }
        let n = self.sort_len as usize;
        let at = match t {
            ArrowType::Toy => 0,
            ArrowType::Normal => {
                if n > 0 && self.sort_set[0] == ArrowType::Toy {
                    1
                } else {
                    0
                }
            }
            _ => n,
        };
        self.sort_set.copy_within(at..n, at + 1);
        self.sort_set[at] = t;
        self.sort_len += 1;
    }

    fn remove_sort_type(&mut self, t: ArrowType) {
        let i = self.sort_index(t);
        if i < 0 {
            return;
        }
        let (i, n) = (i as usize, self.sort_len as usize);
        self.sort_set.copy_within(i + 1..n, i);
        self.sort_len -= 1;
    }

    fn sort(&mut self) {
        let set = self.sort_set;
        let sl = self.sort_len as usize;
        let idx = |t: ArrowType| {
            set[..sl]
                .iter()
                .position(|&s| s == t)
                .map(|i| i as i32)
                .unwrap_or(-1)
        };
        let n = self.len as usize;
        self.arrows[..n].sort_by_key(|&t| -idx(t));
    }

    fn remove_at(&mut self, i: usize) -> ArrowType {
        let n = self.len as usize;
        let t = self.arrows[i];
        self.arrows.copy_within(i + 1..n, i);
        self.len -= 1;
        t
    }

    fn push(&mut self, t: ArrowType) {
        if (self.len as usize) < CAP {
            self.arrows[self.len as usize] = t;
            self.len += 1;
        }
    }

    /// `UseArrow()`: remove and return the next arrow.
    pub fn use_arrow(&mut self) -> Option<ArrowType> {
        if self.len == 0 {
            return None;
        }
        let t = self.remove_at(0);
        if !self.as_slice().contains(&t) {
            self.remove_sort_type(t);
        }
        Some(t)
    }

    /// `AddArrows(types...)`.
    pub fn add_arrows(&mut self, types: &[ArrowType]) {
        for &t in types {
            self.add_sort_type(t);
        }
        for &t in types {
            self.push(t);
        }
        self.sort();
        while self.len > self.max {
            let has_toy = self.as_slice().contains(&ArrowType::Toy);
            let victim = if has_toy {
                self.as_slice().iter().position(|&t| t == ArrowType::Toy)
            } else {
                self.as_slice().iter().position(|&t| t == ArrowType::Normal)
            };
            let i = victim.unwrap_or(self.max as usize);
            let t = self.remove_at(i);
            if !self.as_slice().contains(&t) {
                self.remove_sort_type(t);
            }
        }
    }

    /// `CanAddArrow(t)`.
    pub fn can_add_arrow(&self, t: ArrowType) -> bool {
        if self.len < self.max {
            return true;
        }
        if t.low_priority() {
            return false;
        }
        let ti = self.sort_index(t);
        self.as_slice()
            .iter()
            .any(|&a| a.low_priority() || self.sort_index(a) > ti)
    }

    /// `CanForceAddArrow(t)`: room, or any arrow of another type.
    pub fn can_force_add_arrow(&self, t: ArrowType) -> bool {
        self.len < self.max || self.as_slice().iter().any(|&a| a != t)
    }

    /// `ForceAddArrow(t)` (the Dark World boss laser): append and sort; if
    /// over capacity the first arrow of another type goes (its sort type
    /// stays, as in the original), else the first arrow.
    pub fn force_add_arrow(&mut self, t: ArrowType) {
        self.add_sort_type(t);
        self.push(t);
        self.sort();
        if self.len > self.max {
            match self.as_slice().iter().position(|&a| a != t) {
                Some(i) => {
                    self.remove_at(i);
                }
                None => {
                    self.remove_at(0);
                }
            }
        }
    }

    /// `CanAddArrows(list)`.
    pub fn can_add_arrows(&self, types: &[ArrowType]) -> bool {
        self.len < self.max
            || types
                .iter()
                .any(|&t| !t.low_priority() && self.can_add_arrow(t))
    }

    /// `ToggleSort()`.
    pub fn toggle_sort(&mut self) -> bool {
        let n = self.sort_len as usize;
        if n < 2 {
            return false;
        }
        let last = self.sort_set[n - 1];
        self.sort_set.copy_within(0..n - 1, 1);
        self.sort_set[0] = last;
        self.sort();
        true
    }

    /// `SetMaxArrows(m)`.
    pub fn set_max(&mut self, m: u8) {
        self.max = m;
        while self.len > m {
            self.use_arrow();
        }
    }

    /// `EvenCounts(other)` (team trading).
    pub fn even_counts(&mut self, other: &mut Quiver) -> bool {
        if (self.len as i32 - other.len as i32).abs() <= 1 {
            return false;
        }
        let (big, small) = if self.len > other.len {
            (self, other)
        } else {
            (other, self)
        };
        if let Some(i) = big.as_slice().iter().position(|&t| t == ArrowType::Normal) {
            big.remove_at(i);
            if !big.as_slice().contains(&ArrowType::Normal) {
                big.remove_sort_type(ArrowType::Normal);
            }
            small.add_arrows(&[ArrowType::Normal]);
        } else {
            let i = big.len as usize - 1;
            let t = big.remove_at(i);
            if !big.as_slice().contains(&t) {
                big.remove_sort_type(t);
            }
            small.add_arrows(&[t]);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ArrowType::*;

    #[test]
    fn specials_fire_first_and_toggle() {
        let mut q = Quiver::new(3, Normal);
        q.add_arrows(&[Bomb, Bomb]);
        assert_eq!(q.as_slice(), &[Bomb, Bomb, Normal, Normal, Normal]);
        q.add_arrows(&[Laser]);
        assert_eq!(q.first(), Some(Laser));
        assert!(q.toggle_sort());
        // set was [Normal, Bomb, Laser] -> [Laser, Normal, Bomb]: Bomb first.
        assert_eq!(q.first(), Some(Bomb));
        assert_eq!(q.len(), 6);
        // Full: a Normal cannot be added, a special replaces a Normal.
        assert!(!q.can_add_arrow(Normal));
        assert!(q.can_add_arrow(Drill));
        q.add_arrows(&[Drill]);
        assert_eq!(q.len(), 6);
        assert_eq!(q.as_slice().iter().filter(|&&t| t == Normal).count(), 2);
    }

    #[test]
    fn use_arrow_drops_sort_type() {
        let mut q = Quiver::new(1, Normal);
        q.add_arrows(&[Bomb]);
        assert_eq!(q.use_arrow(), Some(Bomb));
        assert!(!q.toggle_sort());
        assert_eq!(q.use_arrow(), Some(Normal));
        assert_eq!(q.use_arrow(), None);
    }

    #[test]
    fn even_counts() {
        let mut a = Quiver::new(4, Normal);
        let mut b = Quiver::new(1, Normal);
        assert!(a.even_counts(&mut b));
        assert_eq!((a.len(), b.len()), (3, 2));
        assert!(!a.even_counts(&mut b));
    }

    #[test]
    fn force_add_laser_evicts_another_type() {
        // A full quiver of specials takes the laser, evicting one bomb.
        let mut q = Quiver::new(0, Normal);
        q.add_arrows(&[Bomb, Bomb, Bomb, Bomb, Bomb, Bomb]);
        assert!(q.can_force_add_arrow(Laser));
        q.force_add_arrow(Laser);
        assert_eq!(q.len(), 6);
        assert_eq!(q.as_slice().iter().filter(|&&t| t == Laser).count(), 1);
        // All lasers: the first one goes.
        let mut l = Quiver::new(6, Laser);
        assert!(!l.can_force_add_arrow(Laser));
        l.force_add_arrow(Laser);
        assert_eq!(l.len(), 6);
    }
}
