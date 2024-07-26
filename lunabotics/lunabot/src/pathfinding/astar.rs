use std::{cmp::Ordering, collections::BinaryHeap};

use fxhash::FxBuildHasher;
use indexmap::map::Entry::*;
use nalgebra::{convert, Vector2};

type FxIndexMap<K, V> = indexmap::IndexMap<K, V, FxBuildHasher>;

const MAP_DIMENSION: Vector2<u32> = Vector2::new(10, 10);

pub fn astar<FH, FS>(
    mut start: Vector2<f64>,
    step_size: f64,
    mut heuristic: FH,
    mut success: FS,
) -> Vec<Vector2<f64>>
where
    FH: FnMut(Vector2<f64>) -> f64,
    FS: FnMut(Vector2<f64>) -> bool,
{
    let max_index = Vector2::new(
        (MAP_DIMENSION.x as f64 / step_size).round() as u32,
        (MAP_DIMENSION.y as f64 / step_size).round() as u32,
    );
    let mut to_see = BinaryHeap::new();
    to_see.push(SmallestCostHolder {
        estimated_cost: 0,
        cost: 0,
        index: 0,
    });
    let mut parents: FxIndexMap<Vector2<u32>, (usize, usize)> = FxIndexMap::default();
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
    parents.insert(start, (usize::MAX, 0));
    while let Some(SmallestCostHolder { cost, index, .. }) = to_see.pop() {
        let successors = {
            let (node, &(_, c)) = parents.get_index(index).unwrap(); // Cannot fail
            if success(step_size * node.cast()) {
                let path = reverse_path(&parents, index, step_size);
                return path;
            }
            // We may have inserted a node several time into the binary heap if we found
            // a better way to access it. Ensure that we are currently dealing with the
            // best path and discard the others.
            if cost > c {
                continue;
            }

            let mut successors = heapless::Vec::<Vector2<u32>, 4>::new();

            if node.x > 0 {
                successors.push(node - Vector2::new(1, 0)).unwrap();
            }

            if node.y > 0 {
                successors.push(node - Vector2::new(0, 1)).unwrap();
            }

            if node.x < max_index.x {
                successors.push(node + Vector2::new(1, 0)).unwrap();
            }

            if node.y < max_index.y {
                successors.push(node + Vector2::new(0, 1)).unwrap();
            }

            successors
        };
        for successor in successors {
            let new_cost = cost + 1;
            let h; // heuristic(&successor)
            let n; // index for successor
            match parents.entry(successor) {
                Vacant(e) => {
                    h = (heuristic(step_size * e.key().cast()) / step_size).round() as usize;
                    n = e.index();
                    e.insert((index, new_cost));
                }
                Occupied(mut e) => {
                    if e.get().1 > new_cost {
                        h = (heuristic(step_size * e.key().cast()) / step_size).round() as usize;
                        n = e.index();
                        e.insert((index, new_cost));
                    } else {
                        continue;
                    }
                }
            }

            to_see.push(SmallestCostHolder {
                estimated_cost: new_cost + h,
                cost: new_cost,
                index: n,
            });
        }
    }
    todo!()
}

#[derive(Debug, Clone, Copy)]
struct SmallestCostHolder {
    estimated_cost: usize,
    cost: usize,
    index: usize,
}
impl PartialEq for SmallestCostHolder {
    fn eq(&self, other: &Self) -> bool {
        self.estimated_cost.eq(&other.estimated_cost) && self.cost.eq(&other.cost)
    }
}

impl Eq for SmallestCostHolder {}

impl PartialOrd for SmallestCostHolder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SmallestCostHolder {
    fn cmp(&self, other: &Self) -> Ordering {
        match other.estimated_cost.cmp(&self.estimated_cost) {
            Ordering::Equal => self.cost.cmp(&other.cost),
            s => s,
        }
    }
}

fn reverse_path(parents: &FxIndexMap<Vector2<u32>, (usize, usize)>, start: usize, step_size: f64) -> Vec<Vector2<f64>>
{
    let mut i = start;
    let path = std::iter::from_fn(|| {
        parents.get_index(i).map(|(node, &(parent, _cost))| {
            i = parent;
            Vector2::new(
                node.x as f64 * step_size,
                node.y as f64 * step_size,
            )
        })
    })
    .collect::<Vec<Vector2<f64>>>();
    // Collecting the going through the vector is needed to revert the path because the
    // unfold iterator is not double-ended due to its iterative nature.
    path.into_iter().rev().collect()
}