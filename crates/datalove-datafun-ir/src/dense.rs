//! Maps and sets keyed by `ValueId`, as vectors.
//!
//! A code unit numbers its values from zero, so a table of something per
//! value is a vector indexed by the id. The passes that walk a unit build
//! these per pass, and hashing every id was a measurable part of them.

use crate::ValueId;

/// A value for some of a unit's values, by `ValueId`.
#[derive(Clone, Debug)]
pub struct ValueMap<T> {
    slots: Vec<Option<T>>,
    len: usize,
}

impl<T> Default for ValueMap<T> {
    fn default() -> Self {
        ValueMap { slots: Vec::new(), len: 0 }
    }
}

impl<T> ValueMap<T> {
    pub fn get(&self, v: &ValueId) -> Option<&T> {
        self.slots.get(v.0 as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, v: &ValueId) -> Option<&mut T> {
        self.slots.get_mut(v.0 as usize)?.as_mut()
    }

    pub fn contains_key(&self, v: &ValueId) -> bool {
        self.get(v).is_some()
    }

    pub fn insert(&mut self, v: ValueId, value: T) -> Option<T> {
        let old = self.slot(v).replace(value);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    /// The value for `v`, made with `make` if there is none.
    pub fn get_or_insert_with(&mut self, v: ValueId, make: impl FnOnce() -> T) -> &mut T {
        if self.slot(v).is_none() {
            *self.slot(v) = Some(make());
            self.len += 1;
        }
        self.slot(v).as_mut().expect("just filled")
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn slot(&mut self, v: ValueId) -> &mut Option<T> {
        let index = v.0 as usize;
        if self.slots.len() <= index {
            self.slots.resize_with(index + 1, || None);
        }
        &mut self.slots[index]
    }
}

impl<T> std::ops::Index<&ValueId> for ValueMap<T> {
    type Output = T;

    fn index(&self, v: &ValueId) -> &T {
        self.get(v).expect("a value in the map")
    }
}

/// Some of a unit's values.
#[derive(Clone, Debug, Default)]
pub struct ValueSet {
    members: Vec<bool>,
    len: usize,
}

impl ValueSet {
    /// Add `v`, saying whether it was not there already.
    pub fn insert(&mut self, v: ValueId) -> bool {
        let index = v.0 as usize;
        if self.members.len() <= index {
            self.members.resize(index + 1, false);
        }
        let new = !std::mem::replace(&mut self.members[index], true);
        if new {
            self.len += 1;
        }
        new
    }

    /// The members, in id order.
    pub fn iter(&self) -> impl Iterator<Item = ValueId> + '_ {
        self.members.iter().enumerate()
            .filter_map(|(i, &member)| member.then_some(ValueId(i as u32)))
    }

    pub fn contains(&self, v: &ValueId) -> bool {
        self.members.get(v.0 as usize).copied().unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl FromIterator<ValueId> for ValueSet {
    fn from_iter<I: IntoIterator<Item = ValueId>>(iter: I) -> Self {
        let mut set = ValueSet::default();
        for v in iter {
            set.insert(v);
        }
        set
    }
}
