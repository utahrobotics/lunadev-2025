use std::{cmp::Reverse, collections::BinaryHeap, time::Instant};

#[derive(Default, Debug)]
pub struct Blackboard {
    special_instants: BinaryHeap<Reverse<Instant>>,
}

impl Blackboard {
    /// A special instant is an instant that the behavior tree will attempt
    /// to tick on regardless of the target delta.
    ///
    /// For example, if the target delta is 0.3 seconds, and a special
    /// instant was set to 1.05 seconds in the future from now, the
    /// behavior tree will tick at 0.3s, 0.6s, 0.9s, and 1.05s,
    /// then 1.35s, etc.
    pub fn add_special_instant(&mut self, instant: Instant) {
        self.special_instants.push(Reverse(instant));
    }

    pub(super) fn pop_special_instant(&mut self) -> Option<Instant> {
        self.special_instants.pop().map(|Reverse(instant)| instant)
    }

    pub(super) fn peek_special_instant(&mut self) -> Option<&Instant> {
        self.special_instants.peek().map(|Reverse(instant)| instant)
    }
}
