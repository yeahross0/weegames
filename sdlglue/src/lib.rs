#[macro_use]
extern crate c_str_macro;

use cgmath::{prelude::*, vec3, vec4, Deg, Matrix, Matrix4, Vector4};
use gl::types::*;
use sdl2::{
    event::Event,
    image::LoadSurface,
    keyboard::Scancode,
    pixels::{Color as SdlColour, PixelFormat, PixelFormatEnum},
    surface::Surface,
    ttf::Font,
    video::{FullscreenType, Window as SdlWindow},
    EventPump,
};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::Read,
    os::raw::c_void,
    path::Path,
    ptr, str,
};

use shader::Shader;
use wee_common::{Colour, Flip, Rect, Vec2, WeeResult, PROJECTION_HEIGHT, PROJECTION_WIDTH};
#[derive(Debug)]
pub struct Texture {
    pub id: GLuint,
    pub width: GLuint,
    pub height: GLuint,
    pub internal_format: GLuint,
    pub image_format: GLuint,
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.id);
        }
    }
}

impl Texture {
    pub fn init(size: (u32, u32), internal_format: GLenum, image_format: GLenum) -> Texture {
        Texture {
            id: 0,
            width: size.0,
            height: size.1,
            internal_format,
            image_format,
        }
    }

    pub fn text(font: &Font, text: &str, colour: Colour) -> WeeResult<Option<Texture>> {
        let to_sdl = |colour: Colour| {
            let adjust = |c| (c * 255.0) as u8;
            SdlColour::RGBA(
                adjust(colour.r),
                adjust(colour.g),
                adjust(colour.b),
                adjust(colour.a),
            )
        };
        if text.is_empty() {
            Ok(None)
        } else {
            let surface = font
                .render(text)
                .blended(to_sdl(colour))
                .map_err(|e| e.to_string())?;
            let texture = Texture::init((surface.width(), surface.height()), gl::RGBA8, gl::BGRA)
                .format_as(surface, gl::UNSIGNED_INT_8_8_8_8_REV)?;
            Ok(Some(texture))
        }
    }

    fn format_as(mut self, surface: Surface, data_type: GLenum) -> WeeResult<Texture> {
        unsafe {
            gl::GenTextures(1, &mut self.id);
            self.bind();
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            let surface = surface.without_lock().ok_or("Could not get surface")?;
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                self.internal_format as i32,
                self.width as i32,
                self.height as i32,
                0,
                self.image_format,
                data_type,
                surface as *const _ as *const c_void,
            );
            self.unbind();
        }
        Ok(self)
    }

    fn from_surface(mut surface: Surface) -> WeeResult<Texture> {
        let mut texture = Texture::init((surface.width(), surface.height()), gl::RGB, gl::RGB);

        let format = surface.pixel_format();

        unsafe {
            let raw = format.raw();
            if (*raw).format == PixelFormatEnum::ABGR8888 as u32 {
                texture.image_format = gl::RGBA;
                texture.internal_format = gl::RGBA;
                texture.format_as(surface, gl::UNSIGNED_INT_8_8_8_8_REV)
            } else if (*raw).format == PixelFormatEnum::RGB24 as u32 {
                texture.format_as(surface, gl::UNSIGNED_BYTE)
            } else {
                (*raw).format = PixelFormatEnum::RGBA8888 as u32;
                surface = surface.convert(&PixelFormat::from_ll(raw))?;
                texture.image_format = gl::RGBA;
                texture.internal_format = gl::RGBA;
                texture.format_as(surface, gl::UNSIGNED_BYTE)
            }
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> WeeResult<Texture> {
        let surface = Surface::from_file(path)?;

        Texture::from_surface(surface)
    }

    fn bind(&self) {
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.id);
        }
    }

    fn unbind(&self) {
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }
    }
}

pub trait ToVector4 {
    fn to_vec4(self) -> Vector4<f32>;
}

impl ToVector4 for Colour {
    fn to_vec4(self) -> Vector4<f32> {
        vec4(self.r, self.g, self.b, self.a)
    }
}

struct FullScreenInfo {
    recent_change: bool,
    old_size: (u32, u32),
}

pub struct Renderer {
    sprite_shader: Shader,
    rect_shader: Shader,
    quad_vao: u32,
    projection: Matrix4<f32>,
    pub window: SdlWindow,
    full_screen_info: FullScreenInfo,
}

mod shader {
    use super::*;

    /* Adapted from
    https://github.com/bwasty/learn-opengl-rs/blob/master/src/shader.rs
    */
    #[derive(Copy, Clone, Debug)]
    pub struct Shader {
        id: u32,
    }

    /// NOTE: mixture of `shader_s.h` and `shader_m.h` (the latter just contains
    /// a few more setters for uniforms)
    #[allow(dead_code)]
    impl Shader {
        pub fn new(vertex_path: &str, fragment_path: &str) -> Shader {
            let mut shader = Shader { id: 0 };
            // 1. retrieve the vertex/fragment source code from filesystem
            let mut v_shader_file = File::open(vertex_path)
                .unwrap_or_else(|_| panic!("Failed to open {}", vertex_path));
            let mut f_shader_file = File::open(fragment_path)
                .unwrap_or_else(|_| panic!("Failed to open {}", fragment_path));
            let mut vertex_code = String::new();
            let mut fragment_code = String::new();
            v_shader_file
                .read_to_string(&mut vertex_code)
                .expect("Failed to read vertex shader");
            f_shader_file
                .read_to_string(&mut fragment_code)
                .expect("Failed to read fragment shader");

            let v_shader_code = CString::new(vertex_code.as_bytes()).unwrap();
            let f_shader_code = CString::new(fragment_code.as_bytes()).unwrap();

            // 2. compile shaders
            unsafe {
                // vertex shader
                let vertex = gl::CreateShader(gl::VERTEX_SHADER);
                gl::ShaderSource(vertex, 1, &v_shader_code.as_ptr(), ptr::null());
                gl::CompileShader(vertex);
                shader.check_compile_errors(vertex, "VERTEX");
                // fragment Shader
                let fragment = gl::CreateShader(gl::FRAGMENT_SHADER);
                gl::ShaderSource(fragment, 1, &f_shader_code.as_ptr(), ptr::null());
                gl::CompileShader(fragment);
                shader.check_compile_errors(fragment, "FRAGMENT");
                // shader Program
                let id = gl::CreateProgram();
                gl::AttachShader(id, vertex);
                gl::AttachShader(id, fragment);
                gl::LinkProgram(id);
                shader.check_compile_errors(id, "PROGRAM");
                // delete the shaders as they're linked into our program now and no longer necessary
                gl::DeleteShader(vertex);
                gl::DeleteShader(fragment);
                shader.id = id;
            }

            shader
        }

        /// activate the shader
        /// ------------------------------------------------------------------------
        pub unsafe fn use_program(self) {
            gl::UseProgram(self.id)
        }

        /// utility uniform functions
        /// ------------------------------------------------------------------------
        pub unsafe fn set_int(self, name: &CStr, value: i32) {
            gl::Uniform1i(gl::GetUniformLocation(self.id, name.as_ptr()), value);
        }
        pub unsafe fn set_vector4(self, name: &CStr, value: &Vector4<f32>) {
            gl::Uniform4fv(
                gl::GetUniformLocation(self.id, name.as_ptr()),
                1,
                value.as_ptr(),
            );
        }
        /// ------------------------------------------------------------------------
        pub unsafe fn set_mat4(self, name: &CStr, mat: &Matrix4<f32>) {
            gl::UniformMatrix4fv(
                gl::GetUniformLocation(self.id, name.as_ptr()),
                1,
                gl::FALSE,
                mat.as_ptr(),
            );
        }

        /// utility function for checking shader compilation/linking errors.
        /// ------------------------------------------------------------------------
        unsafe fn check_compile_errors(self, shader: u32, type_: &str) {
            let mut success = gl::FALSE as GLint;
            let mut info_log = Vec::with_capacity(1024);
            info_log.set_len(1024 - 1); // subtract 1 to skip the trailing null character
            if type_ != "PROGRAM" {
                gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
                if success != gl::TRUE as GLint {
                    gl::GetShaderInfoLog(
                        shader,
                        1024,
                        ptr::null_mut(),
                        info_log.as_mut_ptr() as *mut GLchar,
                    );
                    log::error!(
                        "SHADER_COMPILATION_ERROR of type: {}\n{}\n \
                     -- --------------------------------------------------- -- ",
                        type_,
                        str::from_utf8(&info_log).unwrap()
                    );
                }
            } else {
                gl::GetProgramiv(shader, gl::LINK_STATUS, &mut success);
                if success != gl::TRUE as GLint {
                    gl::GetProgramInfoLog(
                        shader,
                        1024,
                        ptr::null_mut(),
                        info_log.as_mut_ptr() as *mut GLchar,
                    );
                    log::error!(
                        "PROGRAM_LINKING_ERROR of type: {}\n{}\n \
                     -- --------------------------------------------------- -- ",
                        type_,
                        str::from_utf8(&info_log).unwrap()
                    );
                }
            }
        }
    }
}

impl Renderer {
    pub fn new(window: SdlWindow) -> Renderer {
        let projection = cgmath::ortho(0.0, PROJECTION_WIDTH, PROJECTION_HEIGHT, 0.0, -1.0, 1.0);

        let sprite_shader = {
            let shader = Shader::new("shaders/sprite.vert", "shaders/sprite.frag");

            unsafe {
                shader.use_program();
                shader.set_mat4(c_str!("projection"), &projection);
            }
            shader
        };

        let rect_shader = {
            let shader = Shader::new("shaders/rect.vert", "shaders/rect.frag");

            unsafe {
                shader.use_program();
                shader.set_int(c_str!("image"), 0);
                shader.set_mat4(c_str!("projection"), &projection);
            }

            shader
        };

        fn init() -> u32 {
            let mut quad_vao = 0;
            let mut vbo = 0 as GLuint;
            let vertices: [GLfloat; 24] = [
                // Pos, Pos, Tex, Tex, Pos, Pos, Tex, Tex, ...
                0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0,
                1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0,
            ];

            unsafe {
                gl::GenVertexArrays(1, &mut quad_vao);
                gl::GenBuffers(1, &mut vbo);

                gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
                gl::BufferData(
                    gl::ARRAY_BUFFER,
                    (vertices.len() * std::mem::size_of::<GLfloat>()) as GLsizeiptr,
                    &vertices as *const _ as *const c_void,
                    gl::STATIC_DRAW,
                );

                gl::BindVertexArray(quad_vao);
                gl::EnableVertexAttribArray(0);
                gl::VertexAttribPointer(
                    0,
                    4,
                    gl::FLOAT,
                    gl::FALSE,
                    4 * std::mem::size_of::<GLfloat>() as i32,
                    ptr::null(),
                );
                gl::BindBuffer(gl::ARRAY_BUFFER, 0);
                gl::BindVertexArray(0);
            }

            quad_vao
        }
        let quad_vao = init();

        Renderer::set_viewport(&window);

        let full_screen_info = FullScreenInfo {
            recent_change: false,
            old_size: (0, 0),
        };

        Renderer {
            sprite_shader,
            rect_shader,
            quad_vao,
            projection,
            window,
            full_screen_info,
        }
    }

    pub fn set_viewport(window: &SdlWindow) {
        unsafe {
            let (w, h) = window.size();
            gl::Viewport(0, 0, w as i32, h as i32);
        }
    }

    pub fn update_viewport(&self) {
        Self::set_viewport(&self.window);
    }

    pub fn prepare<'a>(&'a self, texture: &'a Texture) -> TextureDrawer<'a> {
        TextureDrawer::new(self, texture)
    }

    pub fn fill_rectangle(&self, model: Model, colour: Colour) {
        let vertices: [f32; 12] = [1.0, 1.0, 0.5, 1.0, 0.0, 0.5, 0.0, 0.0, 0.5, 0.0, 1.0, 0.5];
        let indices = [0, 1, 3, 1, 2, 3];
        let mut vbo: u32 = 0;
        let mut vao: u32 = 0;
        let mut ebo: u32 = 0;
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::GenBuffers(1, &mut ebo);
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (vertices.len() * std::mem::size_of::<GLfloat>()) as GLsizeiptr,
                &vertices as *const _ as *const c_void,
                gl::STATIC_DRAW,
            );
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (indices.len() * std::mem::size_of::<i32>()) as GLsizeiptr,
                &indices as *const _ as *const c_void,
                gl::STATIC_DRAW,
            );
            gl::VertexAttribPointer(
                0,
                3,
                gl::FLOAT,
                gl::FALSE,
                (3 * std::mem::size_of::<f32>()) as GLsizei,
                ptr::null(),
            );
            gl::EnableVertexAttribArray(0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindVertexArray(0);
            let colour = colour.to_vec4();
            self.rect_shader.use_program();
            self.rect_shader
                .set_mat4(c_str!("projection"), &self.projection);
            self.rect_shader.set_vector4(c_str!("rect_colour"), &colour);

            self.rect_shader.set_mat4(c_str!("model"), &model.0);
            gl::BindVertexArray(vao);
            gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, std::ptr::null());
            gl::DeleteVertexArrays(1, &vao);
            gl::DeleteBuffers(1, &ebo);
            gl::DeleteBuffers(1, &vbo);
        }
    }

    fn draw_texture(
        &self,
        texture: &Texture,
        dest: Rect,
        colour: Colour,
        angle: f32,
        origin: Option<Vec2>,
        flip: Flip,
    ) {
        unsafe {
            self.sprite_shader.use_program();
            self.sprite_shader
                .set_mat4(c_str!("projection"), &self.projection);
        }

        let model = Model::new(dest, origin, angle, flip);

        unsafe {
            let colour = colour.to_vec4();
            self.draw_texture_using_model(texture, model, colour);
        }
    }

    unsafe fn draw_texture_using_model(
        &self,
        texture: &Texture,
        model: Model,
        colour: Vector4<f32>,
    ) {
        self.sprite_shader.set_mat4(c_str!("model"), &model.0);
        self.sprite_shader
            .set_vector4(c_str!("sprite_colour"), &colour);
        gl::ActiveTexture(gl::TEXTURE0);
        texture.bind();
        gl::BindVertexArray(self.quad_vao);
        gl::DrawArrays(gl::TRIANGLES, 0, 6);
        gl::BindVertexArray(0);
        texture.unbind();
    }

    pub fn present(&self) {
        // TODO: Set viewport correctly in the right place
        //unsafe {
        //    gl::Viewport(100, 0, 800, 400);
        //}
        //Renderer::set_viewport(&self.window);
        self.window.gl_swap_window();
    }
}

#[derive(Debug)]
pub struct Model(Matrix4<f32>);

impl Model {
    pub fn new(dest: Rect, origin: Option<Vec2>, angle: f32, flip: Flip) -> Model {
        let size = Vec2::new(dest.w, dest.h);

        let mut model = Matrix4::identity();

        {
            let position = dest.top_left();
            model = model * Matrix4::<f32>::from_translation(vec3(position.x, position.y, 0.0))
        }

        {
            let origin = origin.unwrap_or_else(|| size / 2.0);
            model = model * Matrix4::<f32>::from_translation(vec3(origin.x, origin.y, 0.0));
            model = model * Matrix4::<f32>::from_angle_z(Deg(angle));
            model = model * Matrix4::<f32>::from_translation(vec3(-origin.x, -origin.y, 0.0));
        }

        {
            struct Scaling {
                x: f32,
                y: f32,
                horizontal: f32,
                vertical: f32,
            }
            let mut scaling = Scaling {
                x: size.x,
                y: size.y,
                horizontal: 0.0,
                vertical: 0.0,
            };
            if flip.horizontal {
                scaling.x = -size.x;
                scaling.horizontal = -1.0;
            };
            if flip.vertical {
                scaling.y = -size.y;
                scaling.vertical = -1.0;
            }
            model = model * Matrix4::from_nonuniform_scale(scaling.x, scaling.y, 1.0);
            model = model
                * Matrix4::<f32>::from_translation(vec3(scaling.horizontal, scaling.vertical, 0.0));
        }
        Model(model)
    }
}

#[must_use]
pub struct TextureDrawer<'a> {
    renderer: &'a Renderer,
    texture: &'a Texture,
    dest: Rect,
    colour: Colour,
    angle: f32,
    origin: Option<Vec2>,
    flip: Flip,
}

impl<'a> TextureDrawer<'a> {
    fn new(renderer: &'a Renderer, texture: &'a Texture) -> TextureDrawer<'a> {
        TextureDrawer {
            renderer,
            texture,
            dest: Rect::new(
                PROJECTION_WIDTH / 2.0,
                PROJECTION_HEIGHT / 2.0,
                texture.width as f32,
                texture.height as f32,
            ),
            colour: Colour::white(),
            angle: 0.0,
            origin: None,
            flip: Flip::default(),
        }
    }

    pub fn set_dest(mut self, dest: Rect) -> TextureDrawer<'a> {
        self.dest = dest;
        self
    }

    pub fn set_angle(mut self, angle: f32) -> TextureDrawer<'a> {
        self.angle = angle;
        self
    }

    pub fn set_origin(mut self, origin: Option<Vec2>) -> TextureDrawer<'a> {
        self.origin = origin;
        self
    }

    pub fn flip(mut self, flip: Flip) -> TextureDrawer<'a> {
        self.flip = flip;
        self
    }

    pub fn draw(self) {
        self.renderer.draw_texture(
            self.texture,
            self.dest,
            self.colour,
            self.angle,
            self.origin,
            self.flip,
        );
    }
}

pub fn clear_screen(colour: Colour) {
    unsafe {
        gl::ClearColor(colour.r, colour.g, colour.b, colour.a);
        gl::Clear(gl::COLOR_BUFFER_BIT);
    }
}

pub fn set_fullscreen(renderer: &mut Renderer, event_pump: &EventPump) -> WeeResult<()> {
    if event_pump.keyboard_state().is_scancode_pressed(Scancode::F) {
        if !renderer.full_screen_info.recent_change {
            let display_mode = renderer
                .window
                .subsystem()
                .current_display_mode(renderer.window.display_index()?)?;
            match renderer.window.fullscreen_state() {
                FullscreenType::Off => {
                    renderer.full_screen_info.old_size = renderer.window.size();
                    renderer
                        .window
                        .set_size(display_mode.w as u32, display_mode.h as u32)?;
                    renderer.window.set_fullscreen(FullscreenType::True)?;
                }
                _ => {
                    let size = renderer.full_screen_info.old_size;
                    renderer.window.set_size(size.0, size.1)?;
                    renderer.window.set_fullscreen(FullscreenType::Off)?;
                }
            }
            renderer.full_screen_info.recent_change = true;
        }
    } else {
        renderer.full_screen_info.recent_change = false;
    }
    renderer.update_viewport();
    Ok(())
}

pub fn has_quit(event_pump: &mut EventPump) -> bool {
    for event in event_pump.poll_iter() {
        if let Event::Quit { .. } = event {
            return true;
        }
    }
    false
}
