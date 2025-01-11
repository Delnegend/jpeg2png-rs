use std::time::Duration;

use rayon::prelude::*;
use wide::f32x8;

use crate::{
    compute::{
        aux::Aux, compute_projection_simd::compute_projection_simd,
        compute_step_prob_simd::compute_step_prob_simd,
        compute_step_tv2_simd::compute_step_tv2_simd, compute_step_tv_simd::compute_step_tv_simd,
        f32x8,
    },
    jpeg::Coefficient,
};

#[allow(clippy::too_many_arguments)]
pub fn compute_step_simd(
    max_rounded_px_w: u32,
    max_rounded_px_h: u32,
    max_rounded_px_count: usize,
    nchannel: usize,
    coefs: &[Coefficient],
    auxs: &mut [Aux],
    step_size: f32,
    weight: f32,
    pweight: &[f32; 3],
) {
    for c in 0..nchannel {
        let aux = &mut auxs[c];
        let coef = &coefs[c];

        aux.obj_gradient = vec![0.0; max_rounded_px_count];

        // DCT coefficient distance
        if pweight[c] != 0.0 {
            compute_step_prob_simd(
                max_rounded_px_w,
                max_rounded_px_h,
                pweight[c] * 2.0 * 255.0 * 2.0_f32.sqrt(),
                coef,
                &aux.cos,
                &mut aux.obj_gradient,
            );
        }
    }

    // TV computation
    compute_step_tv_simd(max_rounded_px_w, max_rounded_px_h, nchannel, auxs);

    // TGV second order
    compute_step_tv2_simd(
        max_rounded_px_w,
        max_rounded_px_h,
        nchannel,
        auxs,
        weight / 2.0_f32.sqrt(),
    );

    // Performs a gradient descent step in the direction of the objective gradient
    // with a specified step size. The gradient is normalized before applying the step.
    for aux in auxs.iter_mut() {
        // Calculate Euclidean norm of the objective gradient
        let norm = aux
            .obj_gradient
            .iter()
            .fold(0.0, |acc, &x| acc + x.powi(2))
            .sqrt();

        // Only update if gradient norm is non-zero
        if norm != 0.0 {
            #[cfg(feature = "simd")]
            for i in (0..max_rounded_px_count).step_by(8) {
                let original = &mut aux.fdata[i..i + 8];

                let update = f32x8!(&original[..])
                    - step_size * (f32x8!(&aux.obj_gradient[i..i + 8]) / norm);

                original.copy_from_slice(update.as_array_ref());
            }
        }
    }

    // Project onto DCT basis
    auxs.par_iter_mut().enumerate().for_each(|(c, aux)| {
        compute_projection_simd(max_rounded_px_w, max_rounded_px_h, aux, &coefs[c]);
    });
}
