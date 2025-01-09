mod aux;
mod compute_step;

#[cfg(not(feature = "simd"))]
mod compute_projection;
#[cfg(not(feature = "simd"))]
mod compute_step_prob;
#[cfg(not(feature = "simd"))]
mod compute_step_tv;
#[cfg(not(feature = "simd"))]
mod compute_step_tv2;
#[cfg(not(feature = "simd"))]
use crate::compute::compute_projection::compute_projection;

#[cfg(feature = "simd")]
mod compute_projection_simd;
#[cfg(feature = "simd")]
mod compute_step_prob_simd;
#[cfg(feature = "simd")]
mod compute_step_tv2_simd;
#[cfg(feature = "simd")]
mod compute_step_tv_simd;
#[cfg(feature = "simd")]
use crate::compute::compute_projection_simd::compute_projection_simd as compute_projection;

use rayon::prelude::*;

use crate::{compute::aux::Aux, compute::compute_step::compute_step, jpeg::Coefficient};

#[cfg(feature = "simd")]
macro_rules! f32x8 {
    // Create a f32x8 from a slice with less than 8 elements
    ($fill_range:expr, $slice:expr) => {
        f32x8::from({
            let mut tmp = [0.0; 8];
            tmp[$fill_range].copy_from_slice(&$slice);
            tmp
        })
    };
    // Syntax sugar
    ($slice:expr) => {
        f32x8::from($slice)
    };
    // Syntax sugar
    () => {
        f32x8::splat(0.0)
    };
    // perform simd division if divisor doesn't contain 0 else scalar
    (div: $dividend:expr, $divisor:expr) => {{
        let dividend = $dividend;
        match $divisor.as_array_ref() {
            divisor if divisor.contains(&0.0) => f32x8::from(
                divisor
                    .iter()
                    .enumerate()
                    .map(|(i, g_norm)| match g_norm {
                        0.0 => 0.0,
                        _ => dividend.as_array_ref()[i] / g_norm,
                    })
                    .collect::<Vec<f32>>()
                    .as_slice(),
            ),
            _ => dividend / $divisor,
        }
    }};
}

#[cfg(feature = "simd")]
pub(crate) use f32x8;

pub fn compute(
    nchannel: usize,
    coefs: &mut [Coefficient],
    weight: f32,
    pweight: [f32; 3],
    iterations: u32,
) {
    let max_rounded_px_w = coefs[0].rounded_px_w;
    let max_rounded_px_h = coefs[0].rounded_px_h;
    let max_rounded_px_count = (max_rounded_px_w * max_rounded_px_h) as usize;

    // Initialize working buffers for each channel
    let mut auxs = (0..nchannel)
        .map(|c| {
            Aux::init(
                max_rounded_px_w,
                max_rounded_px_h,
                max_rounded_px_count,
                &coefs[c],
            )
        })
        .collect::<Vec<_>>();

    // Radius of [-0.5, 0.5]^(h*w)
    let radius = (max_rounded_px_count as f32).sqrt() / 2.0;
    let mut term = 1.0_f32;

    // Main iteration loop
    for _i in 0..iterations {
        // FISTA update
        let next_term = (1.0 + (1.0 + 4.0 * term.powi(2)).sqrt()) / 2.0;
        let factor = (term - 1.0) / next_term;

        // Update all channels in parallel
        auxs.par_iter_mut().for_each(|aux| {
            for i in 0..max_rounded_px_count {
                aux.fista[i] = aux.fdata[i] + factor * (aux.fdata[i] - aux.fista[i]);
            }
            std::mem::swap(&mut aux.fdata, &mut aux.fista);
        });

        term = next_term;

        // Take a step
        compute_step(
            max_rounded_px_w,
            max_rounded_px_h,
            max_rounded_px_count,
            nchannel,
            coefs,
            &mut auxs,
            radius / (1.0 + iterations as f32).sqrt(),
            weight,
            &pweight,
        );

        // Project onto DCT basis
        auxs.par_iter_mut().enumerate().for_each(|(c, aux)| {
            compute_projection(max_rounded_px_w, max_rounded_px_h, aux, &coefs[c]);
        });
    }

    // Update coefficients with results
    for c in 0..nchannel {
        coefs[c].rounded_px_w = max_rounded_px_w;
        coefs[c].rounded_px_h = max_rounded_px_h;
        coefs[c].image_data = auxs[c].fdata.clone(); // TODO: might need a Rc/Arc
    }
}
