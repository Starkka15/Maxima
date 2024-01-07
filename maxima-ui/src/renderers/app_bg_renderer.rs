use eframe::egui_glow;
use eframe::glow::{BLEND, TEXTURE_2D};
use egui::mutex::Mutex;
use egui::{TextureId, Vec2};
use egui_glow::glow;
use log::error;
use std::sync::Arc;

/// FUCK
pub struct AppBgRenderer {
    render: Arc<Mutex<ABGUnsafe>>,
}

impl AppBgRenderer {
    pub fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Option<Self> {
        let gl = cc.gl.as_ref()?;
        Some(Self {
            render: Arc::new(Mutex::new(ABGUnsafe::new(gl)?)),
        })
    }

    pub fn draw(&self, ui: &mut egui::Ui, rect: egui::Rect, img_size: Vec2, img: TextureId) {
        let render = self.render.clone();

        let cb = egui_glow::CallbackFn::new(move |_info, painter| {
            render.lock().paint(
                painter.gl(),
                rect.size(),
                img_size,
                painter.texture(img).expect("fuck you"),
            );
        });

        let callback = egui::PaintCallback {
            rect,
            callback: Arc::new(cb),
        };
        ui.painter().add(callback);
    }
}

#[allow(unsafe_code)] //MOM COME PICK ME UP, THEY'RE USING UNSAFE CODE
struct ABGUnsafe {
    //I say this despite having used C++ for years before rust
    program: glow::Program,
}

impl ABGUnsafe {
    fn new(gl: &glow::Context) -> Option<Self> {
        use glow::HasContext as _;

        let glsl_version = egui_glow::ShaderVersion::get(gl);

        unsafe {
            //here we go lmao
            let program = gl
                .create_program()
                .expect("Cannot create OpenGL shader program");
            if !glsl_version.is_new_shader_interface() {
                //uh? not sure what else to do here but fuck you lmao!
                error!("no painting for shader version {:?}", glsl_version);
                return None;
            }

            let vsource = include_str!("../../shaders/abg.vert");
            let fsource = include_str!("../../shaders/abg.frag");

            let shader_src = [
                (glow::VERTEX_SHADER, vsource),
                (glow::FRAGMENT_SHADER, fsource),
            ];

            let shaders: Vec<_> = shader_src
                .iter()
                .map(|(shader_type, shader_source)| {
                    let shader = gl
                        .create_shader(*shader_type)
                        .expect("Cannot create shader");
                    gl.shader_source(
                        shader,
                        &format!("{}\n{}", glsl_version.version_declaration(), shader_source),
                    );
                    gl.compile_shader(shader);
                    assert!(
                        gl.get_shader_compile_status(shader),
                        "Failed to compile ABG {shader_type}: {}",
                        gl.get_shader_info_log(shader)
                    );
                    gl.attach_shader(program, shader);
                    shader
                })
                .collect();

            gl.link_program(program);
            assert!(
                gl.get_program_link_status(program),
                "{}",
                gl.get_program_info_log(program)
            );

            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }

            Some(Self { program: program })
        }
    }

    fn paint(
        &self,
        gl: &glow::Context,
        dimensions: Vec2,
        img_dimensions: Vec2,
        img: glow::Texture,
    ) {
        use glow::HasContext as _;
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_2_f32(
                gl.get_uniform_location(self.program, "u_dimensions")
                    .as_ref(),
                dimensions.x,
                dimensions.y,
            );
            gl.uniform_2_f32(
                gl.get_uniform_location(self.program, "u_img_dimensions")
                    .as_ref(),
                img_dimensions.x,
                img_dimensions.y,
            );
            //gl.uniform_1_u32(self.hero_uniform.as_ref(), TEXTURE_2D);

            gl.bind_texture(TEXTURE_2D, Some(img));

            /*
            gl.bind_vertex_array(Some(self.vert_array));
            */
            gl.enable(BLEND);
            //glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
            //glBlendFuncSeparate();
            /*
            gl.blend_func(glow::FUNC_ADD, glow::ONE_MINUS_SRC_ALPHA);
            */
            //gl.draw_arrays(glow::TRIANGLES, 6, 6);
            gl.draw_arrays(glow::TRIANGLES, 0, 12);
        }
    }
}
