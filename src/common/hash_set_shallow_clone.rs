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
        // TODO: do not invalidate if nothing changed
        self.items.take();
        self.set.remove(item)
    }

    pub fn insert(&mut self, value: T) -> bool {
        // TODO: do not invalidate if nothing changed
        self.items.take();
        self.set.insert(value)
    }

    pub fn _iter(&self) -> hash_set::Iter<T> {
        self.set.iter()
    }
}
