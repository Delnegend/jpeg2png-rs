use std::ops::Deref;

use zune_jpeg::{zune_core::bytestream::ZCursor, JpegDecoder};

use crate::utils::{boxing::unboxing, dct::idct8x8s};

#[derive(Debug, Clone)]
pub enum SampFactor {
    One,
    Two,
}

impl Deref for SampFactor {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        match self {
            SampFactor::One => &1,
            SampFactor::Two => &2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Coefficient {
    /// Rounded up until the next multiple of 8
    pub rounded_px_w: u32,
    /// Rounded up until the next multiple of 8
    pub rounded_px_h: u32,
    pub rounded_px_count: u32,

    /// Result after dividing the pixel width by 8 and rounding up
    pub block_w: u32,
    /// Result after dividing the pixel height by 8 and rounding up
    pub block_h: u32,
    pub block_count: u32,

    pub w_samp_factor: SampFactor,
    pub h_samp_factor: SampFactor,

    pub dct_coefs: Vec<i16>,
    pub image_data: Vec<f32>,
    pub quant_table: [u16; 64],
}

#[derive(Debug)]
pub struct Jpeg {
    pub chan_count: u32,
    pub real_px_w: u32,
    pub real_px_h: u32,
    pub coefs: Vec<Coefficient>,
}

#[derive(Debug, Clone)]
pub enum JpegSource {
    File(String),
    Buffer(Vec<u8>),
}

impl Jpeg {
    pub fn from(jpeg_source: JpegSource) -> Result<Jpeg, String> {
        let buffer = match jpeg_source {
            JpegSource::File(file) => std::fs::read(file).map_err(|e| e.to_string())?,
            JpegSource::Buffer(buffer) => buffer,
        };

        let mut img = JpegDecoder::new(ZCursor::new(&buffer));
        img.decode().map_err(|e| e.to_string())?;

        let (real_px_w, real_px_h) = img
            .dimensions()
            .ok_or("No dimensions")
            .map(|(w, h)| (w as u32, h as u32))?;

        let mut max_w_samp_factor = 0;
        let mut max_h_samp_factor = 0;
        for component in &img.components {
            max_h_samp_factor = max_h_samp_factor.max(component.horizontal_sample);
            max_w_samp_factor = max_w_samp_factor.max(component.vertical_sample);
        }

        Ok(Jpeg {
            chan_count: img.components.len() as u32,
            real_px_w,
            real_px_h,
            coefs: {
                let mut coefs = Vec::with_capacity(img.components.len());
                for comp in img.components {
                    // round to the nearest multiple of 8
                    //
                    // Input: 10
                    // 10 + 7 = 17
                    // 17 in binary     = 0001 0001
                    // !7 in binary     = 1111 1000
                    // 17 & !7
                    let rounded_px_w = ((comp.x + 7) & !7) as u32;
                    let rounded_px_h = ((comp.y + 7) & !7) as u32;
                    let rounded_px_count = rounded_px_w * rounded_px_h;

                    let block_w = rounded_px_w / 8;
                    let block_h = rounded_px_h / 8;
                    let block_count = block_w * block_h;

                    let mut coef = Coefficient {
                        rounded_px_w,
                        rounded_px_h,
                        rounded_px_count,
                        block_w,
                        block_h,
                        block_count,
                        w_samp_factor: match max_w_samp_factor / comp.horizontal_sample {
                            1 => SampFactor::One,
                            2 => SampFactor::Two,
                            _ => return Err("Invalid horizontal sample factor".to_string()),
                        },
                        h_samp_factor: match max_h_samp_factor / comp.vertical_sample {
                            1 => SampFactor::One,
                            2 => SampFactor::Two,
                            _ => return Err("Invalid vertical sample factor".to_string()),
                        },
                        dct_coefs: comp.dct_coefs,
                        image_data: vec![0.0; rounded_px_count as usize],
                        quant_table: comp
                            .quantization_table
                            .iter()
                            .map(|&x| x as u16)
                            .collect::<Vec<u16>>()
                            .try_into()
                            .map_err(|_| "Failed to convert quantization table".to_string())?,
                    };

                    // DCT coefs + quantization table -> image data
                    for i in 0..(block_count as usize) {
                        for j in 0..64 {
                            coef.image_data[i * 64 + j] =
                                coef.dct_coefs[i * 64 + j] as f32 * coef.quant_table[j] as f32;
                        }

                        idct8x8s(
                            coef.image_data[i * 64..(i + 1) * 64]
                                .as_mut()
                                .try_into()
                                .expect("Invalid coef's image data length"),
                        );
                    }

                    // 8x8 -> 64x1
                    unboxing(
                        &coef.image_data.clone(),
                        coef.image_data.as_mut(),
                        coef.rounded_px_w,
                        coef.rounded_px_h,
                        coef.block_w,
                        coef.block_h,
                    );

                    coefs.push(coef);
                }
                coefs
            },
        })
    }
}
