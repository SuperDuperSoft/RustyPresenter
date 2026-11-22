//! A list that copies of the data share. `Data`'s big lists (songs, media files, decks, services) are `Shared`:
//! copying the data (for Undo, for the save, for the operator window's read) then copies a few pointers, not
//! every song. A list is copied only when one holder changes it while another still holds it.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A `Vec` behind a shared pointer. It reads as the `Vec` it holds. Reaching for it mutably (`push`, `iter_mut`,
/// `retain`, an index on the left of `=`) first makes this holder's own copy if another holder shares the list,
/// so no holder ever sees another's change.
pub struct Shared<T>(Arc<Vec<T>>);

impl<T> Shared<T> {
    pub fn new() -> Self { Self(Arc::new(Vec::new())) }

    /// Both hold the very same list (so they are equal, found without comparing an entry).
    pub fn same(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}

impl<T: Clone> Shared<T> {
    /// The list itself, taken out (copied only if another holder shares it).
    pub fn into_vec(self) -> Vec<T> { Arc::try_unwrap(self.0).unwrap_or_else(|shared| (*shared).clone()) }
}

impl<T> Default for Shared<T> {
    fn default() -> Self { Self::new() }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self { Self(self.0.clone()) }
}

impl<T> Deref for Shared<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> { &self.0 }
}

impl<T: Clone> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> { Arc::make_mut(&mut self.0) }
}

impl<T> From<Vec<T>> for Shared<T> {
    fn from(list: Vec<T>) -> Self { Self(Arc::new(list)) }
}

impl<T> FromIterator<T> for Shared<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { Self(Arc::new(iter.into_iter().collect())) }
}

impl<'a, T> IntoIterator for &'a Shared<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter { self.0.iter() }
}

impl<'a, T: Clone> IntoIterator for &'a mut Shared<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter { Arc::make_mut(&mut self.0).iter_mut() }
}

impl<T: PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool { self.same(other) || *self.0 == *other.0 }
}

impl<T: PartialEq> PartialEq<Vec<T>> for Shared<T> {
    fn eq(&self, other: &Vec<T>) -> bool { *self.0 == *other }
}

impl<T: PartialEq> PartialEq<Shared<T>> for Vec<T> {
    fn eq(&self, other: &Shared<T>) -> bool { *self == *other.0 }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

impl<T: Serialize> Serialize for Shared<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { self.0.as_slice().serialize(s) }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Shared<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> { Vec::<T>::deserialize(d).map(Self::from) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_shares_the_list_until_one_of_them_changes_it() {
        let mut a: Shared<String> = vec!["one".to_string(), "two".to_string()].into();
        let b = a.clone();
        assert!(a.same(&b) && a == b && a.len() == 2);
        // Reading shares; the first change makes this holder's own copy and leaves the other as it was.
        assert_eq!(a.iter().map(String::as_str).collect::<Vec<_>>(), ["one", "two"]);
        assert!(a.same(&b));
        a.push("three".into());
        assert!(!a.same(&b) && a != b);
        assert_eq!((a.len(), b.len()), (3, 2));
        a[0] = "ONE".into();
        for s in &mut a { s.push('!') }
        assert_eq!((a[0].as_str(), b[0].as_str()), ("ONE!", "one"));
        // A holder that is alone changes its list in place.
        let mut alone: Shared<u8> = (1..=3).collect();
        let at = alone.as_ptr();
        alone.retain(|n| *n != 2);
        assert_eq!((alone.as_ptr(), alone.clone().into_vec()), (at, vec![1, 3]));
        // Saved and read as the plain list it is.
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, r#"["one","two"]"#);
        assert_eq!(serde_json::from_str::<Shared<String>>(&json).unwrap(), b);
        assert_eq!(b, vec!["one".to_string(), "two".to_string()]);
    }
}
