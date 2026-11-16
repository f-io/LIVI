use crate::clip::{ClipCoverage, MaskCoverage};
use crate::paint::Transfer;
use crate::renderer::Params;

const UNIFORMARRAY_SIZE: usize = 16;

pub struct UniformArray([f32; UNIFORMARRAY_SIZE * 4]);

impl Default for UniformArray {
    fn default() -> Self {
        Self([0.0; UNIFORMARRAY_SIZE * 4])
    }
}

impl UniformArray {
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    pub fn set_scissor_mat(&mut self, mat: [f32; 12]) {
        self.0[0..12].copy_from_slice(&mat);
    }

    pub fn set_paint_mat(&mut self, mat: [f32; 12]) {
        self.0[12..24].copy_from_slice(&mat);
    }

    pub fn set_inner_col(&mut self, col: [f32; 4]) {
        self.0[24..28].copy_from_slice(&col);
    }

    pub fn set_outer_col(&mut self, col: [f32; 4]) {
        self.0[28..32].copy_from_slice(&col);
    }

    pub fn set_scissor_ext(&mut self, ext: [f32; 2]) {
        self.0[32..34].copy_from_slice(&ext);
    }

    pub fn set_scissor_scale(&mut self, scale: [f32; 2]) {
        self.0[34..36].copy_from_slice(&scale);
    }

    pub fn set_scissor_radius(&mut self, radius: f32) {
        self.0[51] = radius;
    }

    pub fn set_extent(&mut self, ext: [f32; 2]) {
        self.0[36..38].copy_from_slice(&ext);
    }

    pub fn set_radius(&mut self, radius: f32) {
        self.0[38] = radius;
    }

    pub fn set_feather(&mut self, feather: f32) {
        self.0[39] = feather;
    }

    pub fn set_stroke_mult(&mut self, stroke_mult: f32) {
        self.0[40] = stroke_mult;
    }

    pub fn set_stroke_thr(&mut self, stroke_thr: f32) {
        self.0[41] = stroke_thr;
    }

    pub fn set_tex_type(&mut self, tex_type: f32) {
        self.0[42] = tex_type;
    }

    pub fn set_shader_type(&mut self, shader_type: f32) {
        self.0[43] = shader_type;
    }

    pub fn set_glyph_texture_type(&mut self, glyph_texture_type: f32) {
        self.0[44] = glyph_texture_type;
    }

    pub fn set_image_blur_filter_direction(&mut self, direction: [f32; 2]) {
        self.0[45..47].copy_from_slice(&direction);
    }

    pub fn set_image_blur_filter_sigma(&mut self, sigma: f32) {
        self.0[47] = sigma;
    }

    pub fn set_image_blur_filter_coeff(&mut self, coeff: [f32; 3]) {
        self.0[48..51].copy_from_slice(&coeff);
    }

    pub fn set_conic_start_angle(&mut self, angle: f32) {
        // frag[13].x in the fragment shader; frag[12].w holds the scissor radius.
        self.0[52] = angle;
    }

    /// The clip shape, read by the shader variant of a draw under one: the
    /// rest of the row the conic angle starts, and the two rows after it.
    pub fn set_clip(&mut self, clip: Option<&ClipCoverage>) {
        if let Some(clip) = clip {
            self.0[54..64].copy_from_slice(&clip.uniform_rows());
        }
    }

    /// The coverage mask, read by the shader variant of a draw under one:
    /// where its corner lies, the pixels it spans and what one of them is
    /// of its image, in the halves of the scissor matrix's columns that no
    /// shader reads, and beside the conic angle whether it takes a pixel
    /// whole or not at all.
    pub fn set_clip_mask(&mut self, mask: Option<&MaskCoverage>) {
        if let Some(mask) = mask {
            self.0[2..4].copy_from_slice(&mask.origin);
            self.0[6..8].copy_from_slice(&mask.size);
            self.0[10..12].copy_from_slice(&mask.texel);
            self.0[53] = f32::from(u8::from(mask.hard));
        }
    }

    /// An image paint's transfer, in the paint matrix's first column's w
    /// (frag[3].w), which no image paint reads. Written only for a paint that
    /// has one: a filter pass keeps its own values in that column.
    pub fn set_image_transfer(&mut self, transfer: Option<Transfer>) {
        if let Some(transfer) = transfer {
            self.0[15] = transfer.uniform();
        }
    }

    /// See [`GRADIENT_PREMULTIPLIED_INDEX`](crate::renderer::params::GRADIENT_PREMULTIPLIED_INDEX).
    pub fn set_gradient_premultiplied(&mut self, premultiplied: bool) {
        if premultiplied {
            self.0[crate::renderer::params::GRADIENT_PREMULTIPLIED_INDEX] = 1.0;
        }
    }
}

impl From<&Params> for UniformArray {
    fn from(params: &Params) -> Self {
        let mut arr = Self::default();

        arr.set_scissor_mat(params.scissor_mat);
        arr.set_paint_mat(params.paint_mat);
        arr.set_inner_col(params.inner_col);
        arr.set_outer_col(params.outer_col);
        arr.set_scissor_ext(params.scissor_ext);
        arr.set_scissor_scale(params.scissor_scale);
        arr.set_scissor_radius(params.scissor_radius);
        arr.set_extent(params.extent);
        arr.set_radius(params.radius);
        arr.set_feather(params.feather);
        arr.set_stroke_mult(params.stroke_mult);
        arr.set_stroke_thr(params.stroke_thr);
        arr.set_shader_type(params.shader_type.to_f32());
        arr.set_tex_type(params.tex_type);
        arr.set_glyph_texture_type(params.glyph_texture_type as f32);
        arr.set_image_blur_filter_direction(params.image_blur_filter_direction);
        arr.set_image_blur_filter_sigma(params.image_blur_filter_sigma);
        arr.set_image_blur_filter_coeff(params.image_blur_filter_coeff);
        arr.set_conic_start_angle(params.conic_start_angle);
        arr.set_clip(params.clip.as_ref());
        arr.set_clip_mask(params.clip_mask.as_ref());
        arr.set_image_transfer(params.image_transfer);
        arr.set_gradient_premultiplied(params.gradient_premultiplied);

        arr
    }
}
