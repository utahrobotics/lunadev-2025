use std::{cmp::Ordering, collections::BinaryHeap};

use fxhash::FxHashMap;
use nalgebra::Vector2;

struct HeapElement {
    node: Vector2<u32>,
    cost: Cost,
}

impl PartialEq for HeapElement {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl Eq for HeapElement {}

impl PartialOrd for HeapElement {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapElement {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cost.cmp(&other.cost)
    }
}

pub(crate) fn astar(
    mut start: Vector2<f64>,
    mut goal: Vector2<f64>,
    map_dimension: Vector2<f64>,
    offset: Vector2<f64>,
    step_size: f64,
    mut is_safe: impl FnMut(Vector2<f64>, Vector2<f64>) -> bool,
) -> Vec<Vector2<f64>> {
    let startf = start;
    let goalf = goal;
    let max_index = Vector2::new(
        (map_dimension.x as f64 / step_size).round() as u32,
        (map_dimension.y as f64 / step_size).round() as u32,
    );

    start -= offset;
    if start.x < 0.0 {
        start.x = 0.0;
    }
    if start.y < 0.0 {
        start.y = 0.0;
    }
    let mut start = Vector2::new(
        (start.x / step_size).round() as u32,
        (start.y / step_size).round() as u32,
    );
    if start.x >= max_index.x {
        start.x = max_index.x;
    }
    if start.y >= max_index.y {
        start.y = max_index.y;
    }

    goal -= offset;
    if goal.x < 0.0 {
        goal.x = 0.0;
    }
    if goal.y < 0.0 {
        goal.y = 0.0;
    }
    let mut goal = Vector2::new(
        (goal.x / step_size).round() as u32,
        (goal.y / step_size).round() as u32,
    );
    if goal.x >= max_index.x {
        goal.x = max_index.x;
    }
    if goal.y >= max_index.y {
        goal.y = max_index.y;
    }
    let heuristic = |node: Vector2<u32>| {
        ((goal.cast::<f64>() - node.cast()).magnitude() * 10.0).round() as usize
    };

    let mut parents: FxHashMap<Vector2<u32>, Parent> = FxHashMap::default();
    parents.insert(start, Parent::Start);
    let mut to_see: BinaryHeap<HeapElement> = BinaryHeap::default();
    let mut best_cost_so_far = Cost {
        heuristic: usize::MAX,
        cost: 0,
        length: 1,
    };
    to_see.push(HeapElement {
        node: start,
        cost: best_cost_so_far,
    });
    let mut best_so_far = start;

    while let Some(HeapElement { node, cost }) = to_see.pop() {
        let successors = {
            if node == goal {
                best_cost_so_far = cost;
                best_so_far = node;
                break;
            } else if cost.heuristic < best_cost_so_far.heuristic {
                best_cost_so_far = cost;
                best_so_far = node;
            }

            let node_parent = parents.get(&node).unwrap();
            let mut successors = heapless::Vec::<_, 8>::new();
            let mut try_add = |next: Vector2<u32>, successor_parent: Parent, cost: usize| {
                if is_safe(
                    step_size * node.cast() + offset,
                    step_size * next.cast() + offset,
                ) {
                    successors.push((next, successor_parent, cost)).unwrap();
                }
            };

            if *node_parent != Parent::NegX && node.x > 0 {
                try_add(node - Vector2::new(1, 0), Parent::PosX, 10);
            }

            if *node_parent != Parent::NegY && node.y > 0 {
                try_add(node - Vector2::new(0, 1), Parent::PosY, 10);
            }

            if *node_parent != Parent::PosX && node.x < max_index.x {
                try_add(node + Vector2::new(1, 0), Parent::NegX, 10);
            }

            if *node_parent != Parent::PosY && node.y < max_index.y {
                try_add(node + Vector2::new(0, 1), Parent::NegY, 10);
            }

            if *node_parent != Parent::NegXNegY && node.x > 0 && node.y > 0 {
                try_add(node - Vector2::new(1, 1), Parent::PosXPosY, 14);
            }

            if *node_parent != Parent::NegXPosY && node.x > 0 && node.y < max_index.y {
                try_add(
                    node - Vector2::new(1, 0) + Vector2::new(0, 1),
                    Parent::PosXNegY,
                    14,
                );
            }

            if *node_parent != Parent::PosXNegY && node.x < max_index.x && node.y > 0 {
                try_add(
                    node + Vector2::new(1, 0) - Vector2::new(0, 1),
                    Parent::NegXPosY,
                    14,
                );
            }

            if *node_parent != Parent::PosXPosY && node.x < max_index.x && node.y < max_index.y {
                try_add(node + Vector2::new(1, 1), Parent::NegXNegY, 14);
            }

            successors
        };

        for (successor, parent, added_cost) in successors {
            let new_cost = cost.cost + added_cost;
            let successor_cost = Cost {
                heuristic: heuristic(successor),
                cost: new_cost,
                length: cost.length + 1,
            };

            to_see.push(HeapElement {
                node: successor,
                cost: successor_cost,
            });
            let _ = parents.try_insert(successor, parent);
        }
    }

    let mut path = vec![goalf; best_cost_so_far.length];

    for i in (1..best_cost_so_far.length - 1).rev() {
        macro_rules! traverse {
            () => {
                best_so_far = match parents.get(&best_so_far).unwrap() {
                    Parent::NegX => best_so_far - Vector2::new(1, 0),
                    Parent::NegY => best_so_far - Vector2::new(0, 1),
                    Parent::PosX => best_so_far + Vector2::new(1, 0),
                    Parent::PosY => best_so_far + Vector2::new(0, 1),
                    Parent::NegXNegY => best_so_far - Vector2::new(1, 1),
                    Parent::NegXPosY => best_so_far - Vector2::new(1, 0) + Vector2::new(0, 1),
                    Parent::PosXNegY => best_so_far + Vector2::new(1, 0) - Vector2::new(0, 1),
                    Parent::PosXPosY => best_so_far + Vector2::new(1, 1),
                    Parent::Start => unreachable!(),
                };
            };
        }
        traverse!();
        path[i] = step_size * best_so_far.cast() + offset;

        #[cfg(debug_assertions)]
        {
            if i == 1 {
                let pre = best_so_far;
                traverse!();
                assert_eq!(
                    best_so_far,
                    start,
                    "pre: {pre:?} {:?}",
                    parents.get(&pre).unwrap()
                );
            }
        }
    }

    path[0] = startf;
    path
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum Parent {
    NegX,
    NegY,
    PosX,
    PosY,
    NegXNegY,
    NegXPosY,
    PosXNegY,
    PosXPosY,
    Start,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Cost {
    heuristic: usize,
    cost: usize,
    length: usize,
}

impl Cost {
    fn estimated_cost(&self) -> usize {
        self.cost + self.heuristic
    }
}

impl PartialOrd for Cost {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cost {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimated_cost()
            .cmp(&self.estimated_cost())
            .then_with(|| other.cost.cmp(&self.cost))
            .then_with(|| other.length.cmp(&self.length))
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_connected_astar() {
//         let path = astar(Vector2::new(0.0, 0.0), Vector2::new(5.0, 0.0), 1.0, |_| {
//             true
//         });
//         assert_eq!(
//             path,
//             vec![
//                 Vector2::new(0.0, 0.0),
//                 Vector2::new(1.0, 0.0),
//                 Vector2::new(2.0, 0.0),
//                 Vector2::new(3.0, 0.0),
//                 Vector2::new(4.0, 0.0),
//                 Vector2::new(5.0, 0.0)
//             ]
//         );
//     }

//     #[test]
//     fn test_disconnected_astar() {
//         let path = astar(Vector2::new(0.0, 0.0), Vector2::new(2.0, 0.0), 1.0, |_| {
//             false
//         });
//         assert_eq!(path, [Vector2::new(0.0, 0.0)]);
//     }

//     #[test]
//     fn test_diagonal_astar() {
//         let path = astar(Vector2::new(0.0, 0.0), Vector2::new(1.0, 1.0), 1.0, |_| {
//             true
//         });
//         assert_eq!(path, [Vector2::new(0.0, 0.0), Vector2::new(1.0, 1.0)]);
//     }

//     #[test]
//     fn test_centered_astar() {
//         let path = astar(
//             Vector2::new(5.0, 5.0),
//             Vector2::new(1.12, 0.83),
//             1.0,
//             |_| true,
//         );
//         assert_eq!(
//             path,
//             [
//                 Vector2::new(5.0, 5.0),
//                 Vector2::new(4.0, 4.0),
//                 Vector2::new(3.0, 3.0),
//                 Vector2::new(2.0, 2.0),
//                 Vector2::new(1.12, 0.83)
//             ]
//         );
//     }

//     #[test]
//     fn test_1_obstacle_astar() {
//         let path = astar(Vector2::new(0.0, 0.0), Vector2::new(5.0, 0.0), 1.0, |p| {
//             p.x != 2.0 || p.y != 0.0
//         });
//         assert_eq!(
//             path,
//             vec![
//                 Vector2::new(0.0, 0.0),
//                 Vector2::new(1.0, 0.0),
//                 Vector2::new(2.0, 1.0),
//                 Vector2::new(3.0, 1.0),
//                 Vector2::new(4.0, 1.0),
//                 Vector2::new(5.0, 0.0)
//             ]
//         );
//     }
// }
