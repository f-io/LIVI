use std::rc::Rc;

use glow::HasContext;

use crate::{renderer::ShaderType, ErrorKind};

const GLSL_VERSION: &str = "#version 100";

pub struct Shader {
    context: Rc<glow::Context>,
    id: <glow::Context as glow::HasContext>::Shader,
}

impl Shader {
    pub fn new(context: &Rc<glow::Context>, src: &str, kind: u32) -> Result<Self, ErrorKind> {
        let id = unsafe { context.create_shader(kind).unwrap() };

        // Compile
        unsafe {
            context.shader_source(id, src);
            context.compile_shader(id);
        }

        // Validate

        let success = unsafe { context.get_shader_compile_status(id) };
        if !success {
            let error = unsafe { context.get_shader_info_log(id) };

            let name = match kind {
                glow::VERTEX_SHADER => "Vertex stage",
                glow::FRAGMENT_SHADER => "Fragment stage",
                _ => "Shader stage",
            };

            return Err(ErrorKind::ShaderCompileError(format!("{name}: {error}")));
        }

        Ok(Self {
            context: context.clone(),
            id,
        })
    }

    pub fn id(&self) -> <glow::Context as glow::HasContext>::Shader {
        self.id
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        unsafe {
            self.context.delete_shader(self.id);
        }
    }
}

#[derive(Debug)]
pub struct Program {
    context: Rc<glow::Context>,
    id: <glow::Context as glow::HasContext>::Program,
}

impl Program {
    pub fn new(context: &Rc<glow::Context>, shaders: &[Shader], attrib_locations: &[&str]) -> Result<Self, ErrorKind> {
        let program = Self {
            context: context.clone(),
            id: unsafe { context.create_program().unwrap() },
        };

        // Attach stages
        for shader in shaders {
            unsafe {
                context.attach_shader(program.id, shader.id());
            }
        }

        for (i, loc) in attrib_locations.iter().enumerate() {
            unsafe {
                context.bind_attrib_location(program.id, i as u32, loc);
            }
        }

        unsafe {
            context.link_program(program.id);
        }

        // Check for error

        let success = unsafe { context.get_program_link_status(program.id) };

        if !success {
            let error = unsafe { context.get_program_info_log(program.id) };

            return Err(ErrorKind::ShaderLinkError(error));
        }

        // Detach stages
        for shader in shaders {
            unsafe {
                context.detach_shader(program.id, shader.id());
            }
        }

        Ok(program)
    }

    pub(crate) fn bind(&self) {
        unsafe {
            self.context.use_program(Some(self.id));
        }
    }

    pub(crate) fn unbind(&self) {
        unsafe {
            self.context.use_program(None);
        }
    }

    fn uniform_location(&self, name: &str) -> Option<<glow::Context as glow::HasContext>::UniformLocation> {
        unsafe { self.context.get_uniform_location(self.id, name) }
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        unsafe {
            self.context.delete_program(self.id);
        }
    }
}

#[derive(Debug)]
pub struct MainProgram {
    context: Rc<glow::Context>,
    program: Program,
    loc_viewsize: <glow::Context as glow::HasContext>::UniformLocation,
    loc_tex: Option<<glow::Context as glow::HasContext>::UniformLocation>,
    loc_glyphtex: Option<<glow::Context as glow::HasContext>::UniformLocation>,
    loc_frag: Option<<glow::Context as glow::HasContext>::UniformLocation>,
}

impl MainProgram {
    pub(crate) fn new(
        context: &Rc<glow::Context>,
        antialias: bool,
        shader_type: ShaderType,
        with_glyph_texture: bool,
    ) -> Result<Self, ErrorKind> {
        let shader_defs = if antialias { "#define EDGE_AA 1" } else { "" };
        let select_shader_type = format!(
            "#define SELECT_SHADER {}\n{}",
            shader_type.to_u8(),
            if with_glyph_texture {
                "#define ENABLE_GLYPH_TEXTURE"
            } else {
                ""
            }
        );
        let vert_shader_src = format!("{}\n{}\n{}", GLSL_VERSION, shader_defs, include_str!("main-vs.glsl"));
        let frag_shader_src = format!(
            "{}\n{}\n{}\n{}",
            GLSL_VERSION,
            shader_defs,
            select_shader_type,
            include_str!("main-fs.glsl")
        );

        let vert_shader = Shader::new(context, &vert_shader_src, glow::VERTEX_SHADER)?;
        let frag_shader = Shader::new(context, &frag_shader_src, glow::FRAGMENT_SHADER)?;

        let program = Program::new(context, &[vert_shader, frag_shader], &["vertex", "tcoord"])?;

        let loc_viewsize = program.uniform_location("viewSize").unwrap();
        let loc_tex = program.uniform_location("tex");
        let loc_glyphtex = program.uniform_location("glyphtex");
        let loc_frag = program.uniform_location("frag");

        Ok(Self {
            context: context.clone(),
            program,
            loc_viewsize,
            loc_tex,
            loc_glyphtex,
            loc_frag,
        })
    }

    pub(crate) fn set_tex(&self, tex: i32) {
        unsafe {
            self.context.uniform_1_i32(self.loc_tex.as_ref(), tex);
        }
    }

    pub(crate) fn set_glyphtex(&self, tex: i32) {
        unsafe {
            self.context.uniform_1_i32(self.loc_glyphtex.as_ref(), tex);
        }
    }

    pub(crate) fn set_view(&self, view: [f32; 2]) {
        unsafe {
            self.context.uniform_2_f32_slice(Some(&self.loc_viewsize), &view);
        }
    }

    pub(crate) fn set_config(&self, config: &[f32]) {
        unsafe {
            self.context.uniform_4_f32_slice(self.loc_frag.as_ref(), config);
        }
    }

    pub(crate) fn bind(&self) {
        self.program.bind();
    }

    pub(crate) fn unbind(&self) {
        self.program.unbind();
    }
}
