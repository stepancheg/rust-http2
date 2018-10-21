use std::collections::hash_set;
use std::collections::HashSet;
use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;
use std::slice;

pub struct HashSetShallowCloneItems<T> {
    items: Option<Rc<Vec<T>>>,
}

impl<T> HashSetShallowCloneItems<T> {
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        match &self.items {
            Some(items) => items.deref().clone(),
            None => Vec::new(),
        }
    }
}

impl<'a, T> IntoIterator for &'a HashSetShallowCloneItems<T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    fn into_iter(self) -> slice::Iter<'a, T> {
        match self.items {
            Some(ref vec) => vec.iter(),
            None => [].iter(),
        }
    }
}

#[derive(Default, Debug, Eq, PartialEq)]
pub struct HashSetShallowClone<T: Hash + Eq + Clone> {
    set: HashSet<T>,
    items: Option<Rc<Vec<T>>>,
}

impl<T: Hash + Eq + Clone> HashSetShallowClone<T> {
    pub fn new() -> Self {
        HashSetShallowClone {
            set: HashSet::new(),
            items: None,
        }
    }

    pub fn items(&mut self) -> HashSetShallowCloneItems<T> {
        // TODO: store delta and update shallow map
        if let None = self.items {
            if self.set.is_empty() {
                // Avoid allocation
                return HashSetShallowCloneItems { items: None };
            }

            self.items = Some(Rc::new(self.set.iter().cloned().collect()));
        }
        HashSetShallowCloneItems {
            items: Some(self.items.as_mut().unwrap().clone()),
        }
    }

    pub fn get(&self, value: &T) -> Option<&T> {
        self.set.get(value)
    }

    pub fn remove(&mut self, item: &T) -> bool {
        let removed = self.set.remove(item);
        if removed {
            self.items.take();
        }
        removed
    }

    pub fn insert(&mut self, value: T) -> bool {
        let inserted = self.set.insert(value);
        if inserted {
            self.items.take();
        }
        inserted
    }

    pub fn _iter(&self) -> hash_set::Iter<T> {
        self.set.iter()
    }
}

#[cfg(test)]
mod test {
    use super::HashSetShallowClone;

    #[test]
    fn insert_updates() {
        let mut s = HashSetShallowClone::new();
        s.insert(10);
        let mut items = s.items().to_vec();
        items.sort();
        assert_eq!(&[10], &items[..]);

        s.insert(20);
        let mut items = s.items().to_vec();
        items.sort();
        assert_eq!(&[10, 20], &items[..]);
    }

    #[test]
    fn remove_updates() {
        let mut s = HashSetShallowClone::new();
        s.insert(10);
        let mut items = s.items().to_vec();
        items.sort();
        assert_eq!(&[10], &items[..]);

        s.insert(20);
        let mut items = s.items().to_vec();
        items.sort();
        assert_eq!(&[10, 20], &items[..]);

        s.remove(&10);
        let mut items = s.items().to_vec();
        items.sort();
        assert_eq!(&[20], &items[..]);
    }

}
