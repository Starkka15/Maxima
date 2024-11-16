use eframe::egui_glow;
use eframe::glow::{BLEND, TEXTURE_2D};
use egui::mutex::Mutex;
use egui::{TextureId, Vec2};
use egui_glow::glow;
use log::error;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::FrontendPerformanceSettings;

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

    pub fn draw(&self, ui: &mut egui::Ui, rect: egui::Rect, img_size: Vec2, img: TextureId, game: f32, flags: FrontendPerformanceSettings) {
        puffin::profile_function!("app background renderer");
        let render = self.render.clone();

        let cb = egui_glow::CallbackFn::new(move |_info, painter| {
            render.lock().paint(
                painter.gl(),
                rect.size(),
                img_size,
                painter.texture(img).expect("fuck you"),
                game,
                flags,
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
    app_dimensions: glow::NativeUniformLocation,
    img_dimensions: glow::NativeUniformLocation,
    flags: glow::NativeUniformLocation,
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

            let app_dimensions = gl.get_uniform_location(program, "u_dimensions");
            let img_dimensions = gl.get_uniform_location(program, "u_img_dimensions");
            let flags = gl.get_uniform_location(program, "u_flags");

            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }

            Some(Self {
                program,
                app_dimensions: app_dimensions.expect("app dimensions uniform not found!"),
                img_dimensions: img_dimensions.expect("image dimensions uniform not found!"),
                flags: flags.expect("flags uniform not found"),
            })
        }
    }

    fn paint(
        &self,
        gl: &glow::Context,
        dimensions: Vec2,
        img_dimensions: Vec2,
        img: glow::Texture,
        game_fade: f32,
        flags: FrontendPerformanceSettings
    ) {
        puffin::profile_function!();
        use glow::HasContext as _;
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_3_f32(Some(&self.app_dimensions), dimensions.x, dimensions.y, game_fade);
            gl.uniform_2_f32(Some(&self.img_dimensions), img_dimensions.x, img_dimensions.y,);
            let mut bitflags: u32 = 0;
            if flags.disable_blur {
                bitflags = bitflags | 0x000001;
            }
            gl.uniform_1_u32(Some(&self.flags), bitflags);

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
