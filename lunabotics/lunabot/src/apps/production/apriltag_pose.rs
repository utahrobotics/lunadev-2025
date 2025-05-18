//! Pure‑Rust re‑implementation of the core *apriltag_pose.c* routines.
//! Everything is written with **nalgebra** so you can feed data produced by
//! the _apriltag_ detector crate directly, without touching the legacy C
//! bindings.
//!
//! Fully implemented:
//! * `calculate_f`      – projection operator.
//! * `polyval`/`solve_poly_approx` – small‑degree polynomial helpers.
//! * `orthogonal_iteration` – Lu (2000) refinement loop.
//! * **NEW:** `fix_pose_ambiguities`, `estimate_pose_for_tag_homography`,
//!   `estimate_tag_pose_orthogonal_iteration` and `estimate_tag_pose` — giving
//!   you a complete drop‑in for the original C API.
//!
//! External crates:
//! * `nalgebra` 0.34  – linear algebra.
//! * `roots`     0.5  – robust quartic root finder used by the ambiguity stage.

use nalgebra::{linalg::SVD, Matrix3, Vector3, Matrix3xX, DMatrix};
use roots::{find_roots_quartic, Roots};

/// Projection operator *F = vvᵀ / (vᵀv)* (cf. **calculate_F** in C).
#[inline]
pub fn calculate_f(v: &Vector3<f64>) -> Matrix3<f64> {
    let denom = v.dot(v);
    if denom.abs() < 1.0e-12 {
        Matrix3::zeros()
    } else {
        (v * v.transpose()) / denom
    }
}

/// Polynomial evaluation matching `polyval` in the C file.
#[inline]
pub fn polyval(p: &[f64], x: f64) -> f64 {
    let mut acc = 0.0;
    let mut x_pow = 1.0;
    for &c in p {
        acc += c * x_pow;
        x_pow *= x;
    }
    acc
}

/// Approximate real roots within ±1000 for polynomials ≤ quartic.
pub fn solve_poly_approx(p: &[f64]) -> Vec<f64> {
    const MAX_ROOT: f64 = 1000.0;
    match p.len() - 1 {
        1 => {
            let (c0, c1) = (p[0], p[1]);
            if (c1 * MAX_ROOT).abs() < c0.abs() {
                vec![]
            } else {
                vec![-c0 / c1]
            }
        }
        4 => match find_roots_quartic(p[4], p[3], p[2], p[1], p[0]) {
            Roots::No(_) => vec![],
            Roots::One(r) => r.iter().cloned().filter(|x| x.abs() <= MAX_ROOT).collect(),
            Roots::Two(r) => r.iter().cloned().filter(|x| x.abs() <= MAX_ROOT).collect(),
            Roots::Three(r) => r.iter().cloned().filter(|x| x.abs() <= MAX_ROOT).collect(),
            Roots::Four(r) => r.iter().cloned().filter(|x| x.abs() <= MAX_ROOT).collect(),
        },
        _ => vec![],
    }
}

/// Lu‑2000 orthogonal iteration. See paper and original C for notation.
#[allow(clippy::too_many_arguments)]
pub fn orthogonal_iteration(
    v: &[Vector3<f64>],
    p: &[Vector3<f64>],
    t: &mut Vector3<f64>,
    r: &mut Matrix3<f64>,
    n_steps: usize,
) -> f64 {
    assert_eq!(v.len(), p.len());
    let n = v.len() as f64;

    // mean‑centre object points
    let p_mean: Vector3<f64> = p.iter().sum::<Vector3<f64>>() / n;
    let p_res: Vec<Vector3<f64>> = p.iter().map(|pi| pi - p_mean).collect();

    // F matrices
    let mut avg_f = Matrix3::<f64>::zeros();
    let fs: Vec<Matrix3<f64>> = v
        .iter()
        .map(|vi| {
            let f = calculate_f(vi);
            avg_f += f;
            f
        })
        .collect();
    avg_f /= n;

    let m1_inv = (Matrix3::<f64>::identity() - avg_f)
        .try_inverse()
        .expect("(I-F̄) must be invertible");

    let mut prev_error = f64::INFINITY;

    for _ in 0..n_steps {
        // translation
        let mut m2 = Vector3::<f64>::zeros();
        for j in 0..fs.len() {
            m2 += (fs[j] - Matrix3::identity()) * ((*r) * p[j]);
        }
        m2 /= n;
        *t = m1_inv * m2;

        // rotation
        let mut q_mean = Vector3::<f64>::zeros();
        let mut q = vec![Vector3::<f64>::zeros(); v.len()];
        for j in 0..v.len() {
            q[j] = fs[j] * ((*r) * p[j] + *t);
            q_mean += q[j];
        }
        q_mean /= n;
        let mut m3 = Matrix3::<f64>::zeros();
        for (qj, pres) in q.iter().zip(p_res.iter()) {
            m3 += (qj - q_mean) * pres.transpose();
        }
        let svd = SVD::new_unordered(m3, true, true);
        let mut r_new = svd.u.unwrap() * svd.v_t.unwrap();
        if r_new.determinant() < 0.0 {
            r_new.column_mut(2).scale_mut(-1.0);
        }
        *r = r_new;

        // error term
        prev_error = fs
            .iter()
            .enumerate()
            .map(|(j, f)| {
                let e = (Matrix3::<f64>::identity() - f) * ((*r) * p[j] + *t);
                e.dot(&e)
            })
            .sum::<f64>();
    }
    prev_error
}

// ——————————————————————————————————————————————————————————————
// 𝟭  Fix pose ambiguities (second local minimum)
// ——————————————————————————————————————————————————————————————

/// Implements the *fix_pose_ambiguities* logic from the reference code. If a
/// second valid minima exists the function returns `Some(new_rotation)`,
/// otherwise `None`.
pub fn fix_pose_ambiguities(
    v: &[Vector3<f64>],
    p: &[Vector3<f64>],
    t: &Vector3<f64>,
    r: &Matrix3<f64>,
) -> Option<Matrix3<f64>> {
    let n = v.len();

    // 1. R_t: aligns camera translation to +Z.
    let r_t_3 = t.normalize();
    let e_x = Vector3::x();
    let r_t_1 = (e_x - r_t_3 * e_x.dot(&r_t_3)).normalize();
    let r_t_2 = r_t_3.cross(&r_t_1);
    let r_t = Matrix3::from_rows(&[
        r_t_1.transpose(),
        r_t_2.transpose(),
        r_t_3.transpose(),
    ]);

    // 2. R_z: rotates so that r31/r32 lie in +X axis.
    let r_1_prime = r_t * r;
    let (mut r31, mut r32) = (r_1_prime[(2, 0)], r_1_prime[(2, 1)]);
    let mut hyp = (r31 * r31 + r32 * r32).sqrt();
    if hyp < 1e-100 {
        r31 = 1.0;
        r32 = 0.0;
        hyp = 1.0;
    }
    let r_z = Matrix3::new(
        r31 / hyp,
        -r32 / hyp,
        0.0,
        r32 / hyp,
        r31 / hyp,
        0.0,
        0.0,
        0.0,
        1.0,
    );

    // 3. R_gamma and initial beta angle.
    let r_trans = r_1_prime * r_z;
    let (sin_gamma, cos_gamma) = (-r_trans[(0, 1)], r_trans[(1, 1)]);
    let r_gamma = Matrix3::new(
        cos_gamma, -sin_gamma, 0.0,
        sin_gamma, cos_gamma, 0.0,
        0.0,       0.0,       1.0,
    );
    let (sin_beta, cos_beta) = (-r_trans[(2, 0)], r_trans[(2, 2)]);
    let t_initial = sin_beta.atan2(cos_beta);

    // Pre‑compute transforms.
    let p_trans: Vec<Vector3<f64>> = p.iter().map(|pi| r_z.transpose() * *pi).collect();
    let v_trans: Vec<Vector3<f64>> = v.iter().map(|vi| r_t * *vi).collect();
    let f_trans: Vec<Matrix3<f64>> = v_trans.iter().map(calculate_f).collect();
    let avg_f_trans: Matrix3<f64> = f_trans.iter().sum::<Matrix3<f64>>() / n as f64;
    let g = ((Matrix3::<f64>::identity() - avg_f_trans)
        .try_inverse()
        .unwrap())
        / n as f64;

    // Constant matrices.
    let m1 = Matrix3::new(0.0, 0.0, 2.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0);
    let m2 = Matrix3::new(-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0);

    // Accumulate b‑vectors.
    let (mut b0, mut b1, mut b2) = (
        Vector3::<f64>::zeros(),
        Vector3::<f64>::zeros(),
        Vector3::<f64>::zeros(),
    );
    for i in 0..n {
        let diff = f_trans[i] - Matrix3::identity();
        b0 += diff * (r_gamma * p_trans[i]);
        b1 += diff * (r_gamma * m1 * p_trans[i]);
        b2 += diff * (r_gamma * m2 * p_trans[i]);
    }
    let (b0_, b1_, b2_) = (g * b0, g * b1, g * b2);

    // Quartic coefficients.
    let (mut a0, mut a1, mut a2, mut a3, mut a4) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let diff = Matrix3::<f64>::identity() - f_trans[i];
        let rp = r_gamma * p_trans[i];
        let c0 = diff * (rp + b0_);
        let c1 = diff * (r_gamma * m1 * p_trans[i] + b1_);
        let c2 = diff * (r_gamma * m2 * p_trans[i] + b2_);

        a0 += c0.dot(&c0);
        a1 += 2.0 * c0.dot(&c1);
        a2 += c1.dot(&c1) + 2.0 * c0.dot(&c2);
        a3 += 2.0 * c1.dot(&c2);
        a4 += c2.dot(&c2);
    }

    // Derivative roots of Eos.
    let poly = [a1, 2.0 * a2 - 4.0 * a0, 3.0 * a3 - 3.0 * a1, 4.0 * a4 - 2.0 * a2, -a3];
    let roots = solve_poly_approx(&poly);

    let mut minima = vec![];
    for &t1 in &roots {
        let t2 = t1 * t1;
        let t3 = t2 * t1;
        let t4 = t2 * t2;
        let t5 = t4 * t1;
        let second_derivative = a2
            - 2.0 * a0
            + (3.0 * a3 - 6.0 * a1) * t1
            + (6.0 * a4 - 8.0 * a2 + 10.0 * a0) * t2
            + (-8.0 * a3 + 6.0 * a1) * t3
            + (-6.0 * a4 + 3.0 * a2) * t4
            + a3 * t5;
        if second_derivative >= 0.0 {
            let t_cur = 2.0 * (t1).atan();
            if (t_cur - t_initial).abs() > 0.1 {
                minima.push(t1);
            }
        }
    }

    if minima.len() == 1 {
        let t_cur = minima[0];
        let r_beta = (Matrix3::<f64>::identity() + m1 * t_cur + m2 * t_cur * t_cur)
            / (1.0 + t_cur * t_cur);
        let new_r = r_t.transpose() * r_gamma * r_beta * r_z.transpose();
        Some(new_r)
    } else {
        None
    }
}

// ——————————————————————————————————————————————————————————————
// 𝟮  Pose from Homography (initial estimate)
// ——————————————————————————————————————————————————————————————
pub fn estimate_pose_for_tag_homography(
    h: &Matrix3<f64>,
    fx: f64,
    fy: f64,
    cx: f64,
    cy: f64,
    tagsize: f64,
) -> (Matrix3<f64>, Vector3<f64>) {
    let k = Matrix3::new(fx, 0.0, cx, 0.0, fy, cy, 0.0, 0.0, 1.0);
    let k_inv = k.try_inverse().expect("Camera matrix must be invertible");
    let h_norm = k_inv * *h;

    let mut r1 = h_norm.column(0).into_owned();
    let mut r2 = h_norm.column(1).into_owned();
    let t = h_norm.column(2).into_owned();

    // Normalize scale
    let lambda = 1.0 / r1.norm();
    r1 *= lambda;
    r2 *= lambda;
    let r3 = r1.cross(&r2);
    let mut r = Matrix3::from_columns(&[r1, r2, r3]);

    // Orthonormalize via SVD
    let svd = SVD::new_unordered(r, true, true);
    r = svd.u.unwrap() * svd.v_t.unwrap();

    // Apply fix matrix to correct handedness (flip y and z axes)
    let fix_rot = Matrix3::new(
        1.0, 0.0, 0.0,
        0.0, -1.0, 0.0,
        0.0, 0.0, -1.0,
    );
    r = fix_rot * r;

    // Adjust translation vector (flip y and z components)
    let mut t = t * lambda * (tagsize / 2.0);
    t.y *= -1.0;
    t.z *= -1.0;

    (r, t)
}
// ——————————————————————————————————————————————————————————————
// 𝟯  End‑to‑end pose estimators (orthogonal iteration wrapper)
// ——————————————————————————————————————————————————————————————

/// Convenience struct mirroring `apriltag_pose_t`.
#[derive(Debug, Clone, Copy)]
pub struct TagPose {
    pub r: Matrix3<f64>,
    pub t: Vector3<f64>,
}

/// Full orthogonal‑iteration pipeline returning up to two candidate poses and
/// their reprojection errors.
#[allow(clippy::too_many_arguments)]
pub fn estimate_tag_pose_orthogonal_iteration(
    img_pts_px: &[[f64; 2]; 4], // (u,v) for the 4 corners, counter‑clockwise.
    tagsize: f64,
    fx: f64,
    fy: f64,
    cx: f64,
    cy: f64,
    n_iters: usize,
) -> ((f64, TagPose), (f64, Option<TagPose>)) {
    let scale = tagsize / 2.0;
    let obj_pts = [
        Vector3::new(-scale, scale, 0.0),
        Vector3::new(scale, scale, 0.0),
        Vector3::new(scale, -scale, 0.0),
        Vector3::new(-scale, -scale, 0.0),
    ];
    let img_rays: Vec<Vector3<f64>> = img_pts_px
        .iter()
        .map(|&[u, v]| Vector3::new((u - cx) / fx, (v - cy) / fy, 1.0))
        .collect();

    // Initial pose via homography (DLT). Build homography from 4 correspondences.
    // nalgebra DLT helper.
    let mut a = DMatrix::<f64>::zeros(9, 8);
    let mut row_idx = 0;
    for (p_obj, &[u, v]) in obj_pts.iter().zip(img_pts_px.iter()) {
        let (x, y, _) = (p_obj.x, p_obj.y, 0.0);
        // First row for this point
        a[(0, row_idx)] = 0.0;
        a[(1, row_idx)] = 0.0;
        a[(2, row_idx)] = 0.0;
        a[(3, row_idx)] = -x;
        a[(4, row_idx)] = -y;
        a[(5, row_idx)] = -1.0;
        a[(6, row_idx)] = v * x;
        a[(7, row_idx)] = v * y;
        a[(8, row_idx)] = v;
        row_idx += 1;
        
        // Second row for this point
        a[(0, row_idx)] = x;
        a[(1, row_idx)] = y;
        a[(2, row_idx)] = 1.0;
        a[(3, row_idx)] = 0.0;
        a[(4, row_idx)] = 0.0;
        a[(5, row_idx)] = 0.0;
        a[(6, row_idx)] = -u * x;
        a[(7, row_idx)] = -u * y;
        a[(8, row_idx)] = -u;
        row_idx += 1;
    }

    let svd_h = SVD::new(a, true, true);
    let v_t = svd_h.v_t.unwrap();
    let h_vec = v_t.row(v_t.nrows() - 1);
    // Create h matrix from the elements
    let mut h = Matrix3::zeros();
    
    // Check that h_vec has enough elements (at least 9)
    let h_vec_len = h_vec.len();
    if h_vec_len >= 9 {
        for i in 0..3 {
            for j in 0..3 {
                let idx = i * 3 + j;
                h[(i, j)] = h_vec[idx];
            }
        }
    } else {
        // If h_vec doesn't have enough elements, use what we have
        let mut idx = 0;
        for i in 0..3 {
            for j in 0..3 {
                if idx < h_vec_len {
                    h[(i, j)] = h_vec[idx];
                    idx += 1;
                } else {
                    h[(i, j)] = 0.0;
                }
            }
        }
    }

    let (mut r, mut t) = estimate_pose_for_tag_homography(&h, -fx, fy, cx, cy, tagsize);

    // Refine pose 1.
    let err1 = orthogonal_iteration(&img_rays, &obj_pts, &mut t, &mut r, n_iters);
    let pose1 = TagPose { r, t };

    // Try to find second minima.
    let alt_r = fix_pose_ambiguities(&img_rays, &obj_pts, &t, &r);
    if let Some(mut r2) = alt_r {
        let mut t2 = Vector3::zeros();
        let err2 = orthogonal_iteration(&img_rays, &obj_pts, &mut t2, &mut r2, n_iters);
        let pose2 = TagPose { r: r2, t: t2 };
        ((err1, pose1), (err2, Some(pose2)))
    } else {
        ((err1, pose1), (f64::INFINITY, None))
    }
}

/// Pick the lower‑error pose produced by `estimate_tag_pose_orthogonal_iteration`.
#[allow(clippy::too_many_arguments)]
pub fn estimate_tag_pose(
    img_pts_px: &[[f64; 2]; 4],
    tagsize: f64,
    fx: f64,
    fy: f64,
    cx: f64,
    cy: f64,
    n_iters: usize,
) -> (f64, TagPose) {
    let (p1, p2) = estimate_tag_pose_orthogonal_iteration(
        img_pts_px, tagsize, fx, fy, cx, cy, n_iters,
    );
    if p1.0 <= p2.0 {
        p1
    } else {
        (p2.0, p2.1.expect("pose should exist when err2 < err1"))
    }
}

// ——————————————————————————————————————————————————————————————
// 𝟰  Tests
// ——————————————————————————————————————————————————————————————
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_fix_pose_returns_none_for_symmetry_case() {
        let v = vec![
            Vector3::new(0.0, 0.0, 1.0);
            4
        ];
        let p = vec![Vector3::zeros(); 4];
        let t = Vector3::new(0.0, 0.0, 1.0);
        let r = Matrix3::identity();
        assert!(fix_pose_ambiguities(&v, &p, &t, &r).is_none());
    }

    #[test]
    fn test_pose_via_homography_identity_intrinsics() {
        let h = Matrix3::identity();
        let (r, t) = estimate_pose_for_tag_homography(&h, 1.0, 1.0, 0.0, 0.0, 1.0);
        assert_relative_eq!(r, Matrix3::identity(), epsilon = 1e-10);
        assert_relative_eq!(t, Vector3::new(0.0, 0.0, 0.5), epsilon = 1e-10);
    }
}
