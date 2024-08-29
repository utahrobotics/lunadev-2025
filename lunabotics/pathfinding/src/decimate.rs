use std::cell::RefCell;

use nalgebra::Vector2;

thread_local! {
    static DECIMATE_BUFFER: RefCell<Vec<Vector2<f64>>> = RefCell::new(Vec::new());
}

/// Simplifies the given path by taking safe shortcuts.
///
/// The capacity of the given vector may change.
pub(crate) fn decimate(
    path: &mut Vec<Vector2<f64>>,
    mut is_safe: impl FnMut(Vector2<f64>, Vector2<f64>) -> bool,
) {
    if path.len() < 3 {
        return;
    }
    DECIMATE_BUFFER.with_borrow_mut(|buffer| {
        buffer.clear();
        let mut from = path[0];
        buffer.push(from);

        loop {
            let mut shortened = false;
            let mut to_index = path.len() - 1;
            let mut to;

            loop {
                to = path[to_index];
                if path[to_index - 1] == from {
                    break;
                }
                if is_safe(from, to) {
                    break;
                }
                to_index -= 1;
                shortened = true
            }

            buffer.push(to);
            from = to;
            if !shortened {
                break;
            }
        }
        std::mem::swap(path, buffer);
    });
}
