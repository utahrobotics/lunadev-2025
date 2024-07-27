use std::cmp::Ordering;

use fxhash::FxHashMap;
use nalgebra::Vector2;

use super::collections::FxPriorityHeapSet;

const MAP_DIMENSION: Vector2<u32> = Vector2::new(10, 10);

pub fn astar(
    mut start: Vector2<f64>,
    mut goal: Vector2<f64>,
    step_size: f64,
    mut is_safe: impl FnMut(Vector2<f64>) -> bool,
) -> Vec<Vector2<f64>> {
    let startf = start;
    let goalf = goal;
    let max_index = Vector2::new(
        (MAP_DIMENSION.x as f64 / step_size).round() as u32,
        (MAP_DIMENSION.y as f64 / step_size).round() as u32,
    );

    if start.x < 0.0 {
        start.x = 0.0;
    }
    if start.y < 0.0 {
        start.y = 0.0;
    }
    let mut start = Vector2::new((start.x / step_size).round() as u32, (start.y / step_size).round() as u32);
    if start.x >= max_index.x {
        start.x = max_index.x;
    }
    if start.y >= max_index.y {
        start.y = max_index.y;
    }

    if goal.x < 0.0 {
        goal.x = 0.0;
    }
    if goal.y < 0.0 {
        goal.y = 0.0;
    }
    let mut goal = Vector2::new((goal.x / step_size).round() as u32, (goal.y / step_size).round() as u32);
    if goal.x >= max_index.x {
        goal.x = max_index.x;
    }
    if goal.y >= max_index.y {
        goal.y = max_index.y;
    }
    let heuristic = |node: Vector2<u32>| (goal.cast::<f64>() - node.cast()).magnitude().round() as usize;

    let mut parents: FxHashMap<Vector2<u32>, Parent> = FxHashMap::default();
    parents.insert(start, Parent::PosX);        // The parent parameter is ignored for start
    let mut to_see: FxPriorityHeapSet<Vector2<u32>, Cost> = FxPriorityHeapSet::default();
    to_see.push_if_higher(start, Cost::default());
    let mut best_so_far = start;
    let mut best_cost_so_far = 0usize;
    let mut best_estimated_cost_so_far = usize::MAX;

    while let Some((node, cost)) = to_see.pop() {
        let successors = {
            if node == goal {
                debug_assert_eq!(node, best_so_far);
                debug_assert_eq!(cost.cost, best_cost_so_far);
                debug_assert_eq!(cost.estimated_cost, best_estimated_cost_so_far);
                break;
            }

            let mut successors = heapless::Vec::<_, 8>::new();
            let mut try_add = |node: Vector2<u32>, parent: Parent| {
                if is_safe(step_size * node.cast()) {
                    successors.push((node, parent)).unwrap();
                }
            };

            if node.x > 0 {
                try_add(node - Vector2::new(1, 0), Parent::PosX);
            }

            if node.y > 0 {
                try_add(node - Vector2::new(0, 1), Parent::PosY);
            }

            if node.x < max_index.x {
                try_add(node + Vector2::new(1, 0), Parent::NegX);
            }

            if node.y < max_index.y {
                try_add(node + Vector2::new(0, 1), Parent::NegY);
            }

            if node.x > 0 && node.y > 0 {
                try_add(node - Vector2::new(1, 1), Parent::PosX);
            }

            if node.x > 0 && node.y < max_index.y {
                try_add(node + Vector2::new(0, 1) - Vector2::new(1, 0), Parent::PosY);
            }

            if node.x < max_index.x && node.y > 0 {
                try_add(node + Vector2::new(1, 0) - Vector2::new(0, 1), Parent::NegX);
            }

            if node.x < max_index.x && node.y < max_index.y {
                try_add(node + Vector2::new(1, 1), Parent::NegY);
            }

            successors
        };

        let new_cost = cost.cost + 1;
        for (successor, parent) in successors {
            let estimated_cost = new_cost + heuristic(successor);
            let cost = Cost {
                estimated_cost,
                cost: new_cost,
            };
            match estimated_cost.cmp(&best_estimated_cost_so_far) {
                Ordering::Less => {
                    best_estimated_cost_so_far = estimated_cost;
                    best_cost_so_far = new_cost;
                    best_so_far = successor;
                }
                Ordering::Equal => {
                    if new_cost > best_cost_so_far {
                        best_cost_so_far = new_cost;
                        best_so_far = successor;
                    }
                }
                _ => {}
            }

            if to_see.push_if_higher(successor, cost) {
                parents.insert(successor, parent);
            }
        }
    }
    
    let mut path = vec![goalf; best_cost_so_far + 1];

    for i in (1..best_cost_so_far).rev() {
        best_so_far = match parents.get(&best_so_far).unwrap() {
            Parent::NegX => best_so_far - Vector2::new(1, 0),
            Parent::NegY => best_so_far - Vector2::new(0, 1),
            Parent::PosX => best_so_far + Vector2::new(1, 0),
            Parent::PosY => best_so_far + Vector2::new(0, 1),
        };
        path[i] = step_size * best_so_far.cast();

        #[cfg(debug_assertions)]
        {
            if i == 1 {
                best_so_far = match parents.get(&best_so_far).unwrap() {
                    Parent::NegX => best_so_far - Vector2::new(1, 0),
                    Parent::NegY => best_so_far - Vector2::new(0, 1),
                    Parent::PosX => best_so_far + Vector2::new(1, 0),
                    Parent::PosY => best_so_far + Vector2::new(0, 1),
                };
                assert_eq!(best_so_far, start);
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
}


#[derive(Debug, Clone, Copy, Default)]
struct Cost {
    estimated_cost: usize,
    cost: usize
}


impl PartialEq for Cost {
    fn eq(&self, other: &Self) -> bool {
        self.estimated_cost.eq(&other.estimated_cost) && self.cost.eq(&other.cost)
    }
}

impl Eq for Cost {}

impl PartialOrd for Cost {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cost {
    fn cmp(&self, other: &Self) -> Ordering {
        match other.estimated_cost.cmp(&self.estimated_cost) {
            Ordering::Equal => self.cost.cmp(&other.cost),
            s => s,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_connected_astar() {
        let path = astar(
            Vector2::new(0.0, 0.0),
            Vector2::new(2.0, 0.0),
            1.0,
            |_| true,
        );
        assert_eq!(path, vec![Vector2::new(0.0, 0.0), Vector2::new(1.0, 0.0), Vector2::new(2.0, 0.0)]);
    }
    
    #[test]
    fn test_disconnected_astar() {
        let path = astar(
            Vector2::new(0.0, 0.0),
            Vector2::new(2.0, 0.0),
            1.0,
            |_| false,
        );
        assert_eq!(path, [Vector2::new(0.0, 0.0)]);
    }
    
    #[test]
    fn test_diagonal_astar() {
        let path = astar(
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 1.0),
            1.0,
            |_| true,
        );
        assert_eq!(path, [Vector2::new(0.0, 0.0), Vector2::new(1.0, 1.0)]);
    }
}