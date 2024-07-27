use std::hash::{BuildHasher, Hash, Hasher, RandomState};

use fxhash::FxBuildHasher;
use num_prime::nt_funcs::next_prime;

const SET_LAMBDA: f64 = 0.5;
const FIRST_PRIME: usize = 19;

#[derive(Debug)]
struct HeapElement<T, P> {
    element: T,
    priority: P,
    set_index: usize
}


impl<T, P: Ord> Ord for HeapElement<T, P> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl<T, P: PartialOrd> PartialOrd for HeapElement<T, P> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.priority.partial_cmp(&other.priority)
    }
}

impl<T, P: PartialEq> PartialEq for HeapElement<T, P> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl<T, P: Eq> Eq for HeapElement<T, P> {}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SetElement {
    Existing(usize),
    Empty,
    Deleted
}

#[derive(Clone, Copy)]
struct HashIndexIter {
    hash: usize,
    i: u32,
    set_length: usize
}


impl HashIndexIter {
    fn new<T: Hash, S: BuildHasher>(element: &T, build_hasher: &S, set_length: usize) -> Self {
        let mut hasher = build_hasher.build_hasher();
        element.hash(&mut hasher);
        // Ignore 32bit platforms
        let hash = hasher.finish() as usize;

        Self {
            hash,
            i: 0,
            set_length
        }
    }
}


impl Iterator for HashIndexIter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let index = (self.hash + 2usize.pow(self.i)) % self.set_length;
        self.i += 1;
        Some(index)
    }
}


pub(super) struct PriorityHeapSet<T, P, S = RandomState> {
    heap: Vec<HeapElement<T, P>>,
    set: Box<[SetElement]>,
    build_hasher: S,
    deleted_slots: usize
}


impl<T, P, S> PriorityHeapSet<T, P, S> {
    pub fn new_with_hasher(build_hasher: S) -> Self {
        Self {
            heap: Vec::with_capacity((FIRST_PRIME as f64 * SET_LAMBDA) as usize),
            set: vec![SetElement::Empty; FIRST_PRIME].into_boxed_slice(),
            build_hasher,
            deleted_slots: 0
        }
    }
}


impl<T: Hash + Eq, P: Ord, S: BuildHasher> PriorityHeapSet<T, P, S> {
    fn push_inner(&mut self, element: T, priority: P) -> Option<OnCollisionArgs<T, P>> {
        if self.set.len() as f64 * SET_LAMBDA < (self.heap.len() + 1 + self.deleted_slots) as f64 {
            self.deleted_slots = 0;
            let next_prime = next_prime(&((self.set.len() as f64 / SET_LAMBDA) as usize), None).expect("Overflow while finding prime");
            self.set = vec![SetElement::Empty; next_prime].into_boxed_slice();

            for heap_index in 0..self.heap.len() {
                let new_set_index = 'main: {
                    for set_index in HashIndexIter::new(&self.heap[heap_index].element, &self.build_hasher, self.set.len()) {
                        if self.set[set_index] == SetElement::Empty {
                            break 'main set_index;
                        }
                    }
                    unreachable!()
                };
                self.set[new_set_index] = SetElement::Existing(heap_index);
                self.heap[heap_index].set_index = new_set_index;
            }
        }

        for set_index in HashIndexIter::new(&element, &self.build_hasher, self.set.len()) {
            match self.set[set_index] {
                SetElement::Existing(heap_index) => {
                    if self.heap[heap_index].element == element {
                        return Some(OnCollisionArgs { set_index, heap_index, element, new_priority: priority });
                    }
                }
                SetElement::Deleted => {
                    // continue searching because we need to confirm that the element
                    // is not in the set
                }
                SetElement::Empty => {
                    let mut heap_index = self.heap.len();
                    self.heap.push(HeapElement { element, priority, set_index });
                    heap_index = self.percolate_up(heap_index);
                    self.set[set_index] = SetElement::Existing(heap_index);
                    break;
                }
            }
        }
        None
    }

    fn percolate_up(&mut self, mut heap_index: usize) -> usize {
        while heap_index > 0 {
            let element = &self.heap[heap_index];
            let parent_index = (heap_index - 1) / 2;
            let parent_element = &self.heap[parent_index];
            
            if element > parent_element {
                self.set[parent_element.set_index] = SetElement::Existing(heap_index);
                self.heap.swap(parent_index, heap_index);
                heap_index = parent_index;
            } else {
                break;
            }
        }
        heap_index
    }

    fn percolate_down(&mut self, mut heap_index: usize) {
        let set_index = self.heap[heap_index].set_index;

        loop {
            let left_child_index = heap_index * 2 + 1;
            let right_child_index = left_child_index + 1;
            let current = &self.heap[heap_index];

            if left_child_index >= self.heap.len() {
                // No children
                break;
            } else if right_child_index >= self.heap.len() {
                // Only left child
                if self.heap[left_child_index] > *current {
                    self.set[self.heap[left_child_index].set_index] = SetElement::Existing(heap_index);
                    self.heap.swap(left_child_index, heap_index);
                    heap_index = left_child_index;
                } else {
                    break;
                }
            } else {
                // Both children
                let left_child = &self.heap[left_child_index];
                let right_child = &self.heap[right_child_index];

                if left_child > right_child {
                    if left_child > current {
                        self.set[self.heap[left_child_index].set_index] = SetElement::Existing(heap_index);
                        self.heap.swap(left_child_index, heap_index);
                        heap_index = left_child_index;
                    } else {
                        break;
                    }
                } else if right_child > current {
                    self.set[self.heap[right_child_index].set_index] = SetElement::Existing(heap_index);
                    self.heap.swap(right_child_index, heap_index);
                    heap_index = right_child_index;
                } else {
                    break;
                }
            }
        }

        self.set[set_index] = SetElement::Existing(heap_index);
    }

    pub fn push_if_higher(&mut self, element: T, new_priority: P) -> bool {
        if let Some(OnCollisionArgs { set_index, mut heap_index, element, new_priority }) = self.push_inner(element, new_priority) {
            let old_element = &mut self.heap[heap_index];
            if new_priority <= old_element.priority {
                return false;
            }
            old_element.element = element;
            old_element.priority = new_priority;
            heap_index = self.percolate_up(heap_index);
            self.set[set_index] = SetElement::Existing(heap_index);
        }
        true
    }

    pub fn pop(&mut self) -> Option<(T, P)> {
        if self.heap.is_empty() {
            return None;
        }

        let HeapElement { element, priority, set_index } = self.heap.swap_remove(0);
        self.set[set_index] = SetElement::Deleted;
        self.deleted_slots += 1;

        if !self.heap.is_empty() {
            self.percolate_down(0);
        }

        Some((element, priority))
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}


impl<T, P, S: Default> Default for PriorityHeapSet<T, P, S> {
    fn default() -> Self {
        Self::new_with_hasher(S::default())
    }
}


struct OnCollisionArgs<T, P> {
    set_index: usize,
    heap_index: usize,
    element: T,
    new_priority: P
}

pub(super) type FxPriorityHeapSet<T, P> = PriorityHeapSet<T, P, FxBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_pop() {
        let mut heap_set: FxPriorityHeapSet<u32, u32> = FxPriorityHeapSet::default();

        heap_set.push_if_higher(1, 10);
        heap_set.push_if_higher(2, 5);
        heap_set.push_if_higher(3, 15);
        heap_set.push_if_higher(4, 8);

        assert_eq!(heap_set.len(), 4);

        assert_eq!(heap_set.pop(), Some((3, 15)));
        assert_eq!(heap_set.pop(), Some((1, 10)));
        assert_eq!(heap_set.pop(), Some((4, 8)));
        assert_eq!(heap_set.pop(), Some((2, 5)));

        assert_eq!(heap_set.len(), 0);
        assert!(heap_set.is_empty());
    }

    #[test]
    fn test_push_if_higher() {
        let mut heap_set: FxPriorityHeapSet<u32, u32> = FxPriorityHeapSet::default();

        heap_set.push_if_higher(1, 10);
        heap_set.push_if_higher(2, 5);
        heap_set.push_if_higher(3, 15);
        heap_set.push_if_higher(4, 8);

        assert_eq!(heap_set.len(), 4);

        heap_set.push_if_higher(2, 20);
        heap_set.push_if_higher(4, 12);

        assert_eq!(heap_set.len(), 4);

        assert_eq!(heap_set.pop(), Some((2, 20)));
        assert_eq!(heap_set.pop(), Some((3, 15)));
        assert_eq!(heap_set.pop(), Some((4, 12)));
        assert_eq!(heap_set.pop(), Some((1, 10)));

        assert_eq!(heap_set.len(), 0);
        assert!(heap_set.is_empty());
    }

    #[test]
    fn test_push_and_pop_with_duplicates() {
        let mut heap_set: FxPriorityHeapSet<u32, u32> = FxPriorityHeapSet::default();

        heap_set.push_if_higher(1, 10);
        heap_set.push_if_higher(2, 5);
        heap_set.push_if_higher(3, 15);
        heap_set.push_if_higher(4, 8);
        heap_set.push_if_higher(2, 20);
        heap_set.push_if_higher(4, 12);

        assert_eq!(heap_set.len(), 4);

        assert_eq!(heap_set.pop(), Some((2, 20)));
        assert_eq!(heap_set.pop(), Some((3, 15)));
        assert_eq!(heap_set.pop(), Some((4, 12)));
        assert_eq!(heap_set.pop(), Some((1, 10)));

        assert_eq!(heap_set.len(), 0);
        assert!(heap_set.is_empty());
    }

    #[test]
    fn test_push_and_pop_with_same_priority() {
        let mut heap_set: FxPriorityHeapSet<u32, u32> = FxPriorityHeapSet::default();

        heap_set.push_if_higher(1, 10);
        heap_set.push_if_higher(2, 5);
        heap_set.push_if_higher(3, 15);
        heap_set.push_if_higher(4, 8);
        heap_set.push_if_higher(5, 10);

        assert_eq!(heap_set.len(), 5);

        assert_eq!(heap_set.pop(), Some((3, 15)));
        assert_eq!(heap_set.pop(), Some((1, 10)));
        assert_eq!(heap_set.pop(), Some((5, 10)));
        assert_eq!(heap_set.pop(), Some((4, 8)));
        assert_eq!(heap_set.pop(), Some((2, 5)));

        assert_eq!(heap_set.len(), 0);
        assert!(heap_set.is_empty());
    }
}
