use std::collections::hash_set;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

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

    pub fn items(&mut self) -> Rc<Vec<T>> {
        // TODO: store delta and update shallow map
        if let None = self.items {
            self.items = Some(Rc::new(self.set.iter().cloned().collect()));
        }
        self.items.as_mut().unwrap().clone()
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
    use std::ops::Deref;

    #[test]
    fn insert_updates() {
        let mut s = HashSetShallowClone::new();
        s.insert(10);
        let mut items = s.items().deref().clone();
        items.sort();
        assert_eq!(&[10], &items[..]);

        s.insert(20);
        let mut items = s.items().deref().clone();
        items.sort();
        assert_eq!(&[10, 20], &items[..]);
    }

    #[test]
    fn remove_updates() {
        let mut s = HashSetShallowClone::new();
        s.insert(10);
        let mut items = s.items().deref().clone();
        items.sort();
        assert_eq!(&[10], &items[..]);

        s.insert(20);
        let mut items = s.items().deref().clone();
        items.sort();
        assert_eq!(&[10, 20], &items[..]);

        s.remove(&10);
        let mut items = s.items().deref().clone();
        items.sort();
        assert_eq!(&[20], &items[..]);
    }

}
