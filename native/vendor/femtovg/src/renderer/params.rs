use crate::{
    geometry::Position,
    paint::{GlyphTexture, GradientColors},
    ImageFlags, ImageStore, PaintFlavor, PixelFormat, Scissor, Transform2D,
};

use super::ShaderType;

#[derive(Copy, Clone, Debug, Default)]
pub struct Params {
    pub(crate) scissor_mat: [f32; 12],
    pub(crate) paint_mat: [f32; 12],
    pub(crate) inner_col: [f32; 4],
    pub(crate) outer_col: [f32; 4],
    pub(crate) scissor_ext: [f32; 2],
    pub(crate) scissor_scale: [f32; 2],
    pub(crate) scissor_radius: f32,
    pub(crate) extent: [f32; 2],
    pub(crate) radius: f32,
    pub(crate) feather: f32,
    pub(crate) stroke_mult: f32,
    pub(crate) stroke_thr: f32,
    pub(crate) tex_type: f32,
    pub(crate) shader_type: ShaderType,
    pub(crate) glyph_texture_type: u8, // 0 -> no glyph rendering, 1 -> alpha mask, 2 -> color texture
    pub(crate) image_blur_filter_direction: [f32; 2],
    pub(crate) image_blur_filter_sigma: f32,
    pub(crate) image_blur_filter_coeff: [f32; 3],
    pub(crate) conic_start_angle: f32,
}

impl Params {
    pub(crate) fn new<T>(
        images: &ImageStore<T>,
        global_transform: &Transform2D,
        paint_flavor: &PaintFlavor,
        glyph_texture: &GlyphTexture,
        scissor: &Scissor,
        stroke_width: f32,
        fringe_width: f32,
        stroke_thr: f32,
    ) -> Self {
        let mut params = Self::default();

        // Scissor
        let (scissor_ext, scissor_scale) = if let Some(ext) = scissor.extent {
            if ext[0] < -0.5 || ext[1] < -0.5 {
                ([1.0, 1.0], [1.0, 1.0])
            } else {
                params.scissor_mat = scissor.transform.inverse().to_mat3x4();

                let Transform2D([a, b, c, d, ..]) = scissor.transform;
                let scissor_scale = [a.hypot(c) / fringe_width, b.hypot(d) / fringe_width];

                (ext, scissor_scale)
            }
        } else {
            ([1.0, 1.0], [1.0, 1.0])
        };

        params.scissor_ext = scissor_ext;
        params.scissor_scale = scissor_scale;
        params.scissor_radius = scissor.radius;

        params.stroke_mult = (stroke_width * 0.5 + fringe_width * 0.5) / fringe_width;
        params.stroke_thr = stroke_thr;

        params.glyph_texture_type = match glyph_texture {
            GlyphTexture::None => 0,
            GlyphTexture::AlphaMask(_) => 1,
            GlyphTexture::ColorTexture(_) => 2,
        };

        let inv_transform;

        match &paint_flavor {
            PaintFlavor::Color(color) => {
                let color = color.premultiplied().to_array();
                params.inner_col = color;
                params.outer_col = color;
                params.shader_type = ShaderType::FillColor;
                inv_transform = global_transform.inverse();
            }
            &PaintFlavor::Image {
                id,
                center: Position { x: cx, y: cy },
                width,
                height,
                angle,
                tint,
            } => {
                let Some(image_info) = images.info(*id) else {
                    return params;
                };

                params.extent[0] = *width;
                params.extent[1] = *height;

                let color = tint;

                params.inner_col = color.premultiplied().to_array();
                params.outer_col = color.premultiplied().to_array();

                let mut transform = Transform2D::rotation(*angle);
                transform.translate(*cx, *cy);
                transform *= *global_transform;

                if image_info.flags().contains(ImageFlags::FLIP_Y) {
                    let mut m1 = Transform2D::translation(0.0, height * 0.5);
                    m1 *= transform;

                    let mut m2 = Transform2D::scaling(1.0, -1.0);
                    m2 *= m1;

                    let mut m1 = Transform2D::translation(0.0, -height * 0.5);
                    m1 *= m2;

                    inv_transform = m1.inverse();
                } else {
                    inv_transform = transform.inverse();
                }

                params.shader_type = ShaderType::FillImage;

                params.tex_type = match image_info.format() {
                    PixelFormat::Rgba8 => {
                        if image_info.flags().contains(ImageFlags::PREMULTIPLIED) {
                            0.0
                        } else {
                            1.0
                        }
                    }
                    PixelFormat::Gray8 => 2.0,
                    PixelFormat::Rgb8 => 0.0,
                };
            }
            PaintFlavor::LinearGradient {
                start: Position { x: start_x, y: start_y },
                end: Position { x: end_x, y: end_y },
                colors,
            } => {
                let large = 1e5f32;
                let mut dx = end_x - start_x;
                let mut dy = end_y - start_y;
                let d = dx.hypot(dy);

                if d > 0.0001 {
                    dx /= d;
                    dy /= d;
                } else {
                    dx = 0.0;
                    dy = 1.0;
                }

                let mut transform = Transform2D([dy, -dx, dx, dy, start_x - dx * large, start_y - dy * large]);

                transform *= *global_transform;

                inv_transform = transform.inverse();

                params.extent[0] = large;
                params.extent[1] = large + d * 0.5;
                params.feather = 1.0f32.max(d);

                match colors {
                    GradientColors::TwoStop { start_color, end_color } => {
                        params.inner_col = start_color.premultiplied().to_array();
                        params.outer_col = end_color.premultiplied().to_array();
                        params.shader_type = ShaderType::FillGradient;
                    }
                    GradientColors::MultiStop { .. } => {
                        params.shader_type = ShaderType::FillImageGradient;
                    }
                }
            }
            &PaintFlavor::BoxGradient {
                pos: Position { x, y },
                width,
                height,
                radius,
                feather,
                colors,
            } => {
                let mut transform = Transform2D::translation(x + width * 0.5, y + height * 0.5);
                transform *= *global_transform;
                inv_transform = transform.inverse();

                params.extent[0] = width * 0.5;
                params.extent[1] = height * 0.5;
                params.radius = *radius;
                params.feather = *feather;
                match colors {
                    GradientColors::TwoStop { start_color, end_color } => {
                        params.inner_col = start_color.premultiplied().to_array();
                        params.outer_col = end_color.premultiplied().to_array();
                        params.shader_type = ShaderType::FillGradient;
                    }
                    GradientColors::MultiStop { .. } => {
                        params.shader_type = ShaderType::FillImageGradient;
                    }
                }
            }
            &PaintFlavor::RadialGradient {
                center: Position { x: cx, y: cy },
                in_radius: (in_rx, in_ry),
                out_radius: (out_rx, out_ry),
                colors,
            } => {
                let avg_x = (in_rx + out_rx) * 0.5;
                let avg_y = (in_ry + out_ry) * 0.5;
                let effective_r = (avg_x + avg_y) * 0.5;
                let f = ((out_rx - in_rx) + (out_ry - in_ry)) * 0.5;

                // squash into an ellipse
                let mut transform = Transform2D::scaling(avg_x / effective_r, avg_y / effective_r);
                transform.translate(*cx, *cy);
                transform *= *global_transform;
                inv_transform = transform.inverse();

                params.extent[0] = effective_r;
                params.extent[1] = effective_r;
                params.radius = effective_r;
                params.feather = 1.0f32.max(f);
                match colors {
                    GradientColors::TwoStop { start_color, end_color } => {
                        params.inner_col = start_color.premultiplied().to_array();
                        params.outer_col = end_color.premultiplied().to_array();
                        params.shader_type = ShaderType::FillGradient;
                    }
                    GradientColors::MultiStop { .. } => {
                        params.shader_type = ShaderType::FillImageGradient;
                    }
                }
            }
            &PaintFlavor::ConicGradient {
                center: Position { x: cx, y: cy },
                start_angle,
                colors,
            } => {
                let mut transform = Transform2D::translation(*cx, *cy);
                transform *= *global_transform;
                inv_transform = transform.inverse();

                params.conic_start_angle = *start_angle;

                match colors {
                    GradientColors::TwoStop { start_color, end_color } => {
                        params.inner_col = start_color.premultiplied().to_array();
                        params.outer_col = end_color.premultiplied().to_array();
                        params.shader_type = ShaderType::FillGradientConic;
                    }
                    GradientColors::MultiStop { .. } => {
                        params.shader_type = ShaderType::FillImageGradientConic;
                    }
                }
            }
        }

        params.paint_mat = inv_transform.to_mat3x4();

        params
    }

    pub(crate) fn uses_glyph_texture(self) -> bool {
        self.glyph_texture_type != 0
    }
}
