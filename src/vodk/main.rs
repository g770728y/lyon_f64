#![crate_id = "vodk#0.1"]
#![feature(macro_rules, globs)]
#![feature(default_type_params)]

extern crate native;
extern crate gl;
extern crate glfw;
extern crate time;
extern crate png;

use gfx::renderer;
use gfx::shaders;
use gfx::text;
use gfx::window;
use math::vector;
use io::inputs;

use std::io::timer::sleep;

pub mod gfx {
    pub mod renderer;
    pub mod opengl;
    pub mod window;
    pub mod shaders;
    pub mod mesh_utils;
    pub mod geom;
    pub mod text;
    pub mod test_renderer;
}
pub mod logic {
    pub mod entity;
}
pub mod base {
	pub mod containers;
}
pub mod data {
    //pub mod layout;
}
pub mod kiwi {
    pub mod graph;
}

pub mod math {
    pub mod vector;
}

pub mod io {
    pub mod inputs;
}

pub mod app;

struct TestApp {
    shaders: Vec<(renderer::ShaderProgram, UniformLayout)>,
    draw_calls: Vec<(renderer::TargetTypes, renderer::GeometryRange)>,
    textures: Vec<renderer::Texture>,
    ctx: Box<renderer::RenderingContext>,
}

impl app::Application for TestApp {
    fn handle_events(&mut self, events: &[inputs::Event]) {

    }

    fn update(&mut self, dt: f32, frame_count: u64) {
        let ctx: &mut renderer::RenderingContext = self.ctx;
        let screen = ctx.get_default_render_target();
        ctx.set_render_target(screen);
        ctx.clear(renderer::COLOR|renderer::DEPTH);


        let &(shader, uniforms) = self.shaders.get(0);
        let &(targets, geom_range) = self.draw_calls.get(0);
        ctx.set_shader(shader).ok().expect("set texturing shader");
        ctx.set_shader_input_texture(uniforms.u_texture_0, 0, *self.textures.get(0));
        ctx.set_shader_input_float(uniforms.u_resolution, [800.0, 600.0]);
        ctx.draw(geom_range, targets).ok().expect("draw(checker texture)");


        let &(shader, uniforms) = self.shaders.get(1);
        let &(targets, geom_range) = self.draw_calls.get(1);
        ctx.set_shader(shader).ok().expect("set text shader");
        ctx.set_shader_input_float(uniforms.u_color, [1.0, 0.0, 0.0, 1.0]);
        ctx.set_shader_input_texture(uniforms.u_texture_0, 0, *self.textures.get(1));
        ctx.draw(geom_range, targets).ok().expect("draw(text)");


        let &(shader, uniforms) = self.shaders.get(2);
        let &(targets, geom_range) = self.draw_calls.get(2);
        ctx.set_shader(shader).ok().expect("set 3d shader");
        let mut proj_mat = vector::Mat4::identity();
        vector::Mat4::perspective(45.0, 1.5, 0.5, 1000.0, &mut proj_mat);

        let mut model_mat = vector::Mat4::identity();
        let mut view_mat = vector::Mat4::identity();
        view_mat.translate(&vector::vec3(0.0, 0.0, -10.0));
        view_mat.rotate(vector::PI * (frame_count as f32 * 0.01).sin(), &vector::vec3(0.0, 1.0, 0.0));

        ctx.set_shader_input_matrix(uniforms.u_model_mat, model_mat.as_slice(), 4, false);
        ctx.set_shader_input_matrix(uniforms.u_view_mat, view_mat.as_slice(), 4, false);
        ctx.set_shader_input_matrix(uniforms.u_proj_mat, proj_mat.as_slice(), 4, false);

        ctx.draw(geom_range, targets).ok().expect("draw(checker texture)");

        ctx.swap_buffers();
    }

    fn get_help(&self) -> String { "Vodk!".to_string() }

    fn shut_down(&mut self) {
        println!("bye");
    }
}

impl TestApp {
    fn new(
        window: &mut window::Window,
        ctx: Box<renderer::RenderingContext>
    ) -> TestApp {
        TestApp {
            textures: Vec::new(),
            draw_calls: Vec::new(),
            shaders: Vec::new(),
            ctx: ctx,
        }
    }

    fn init(&mut self) {
        let ctx: &mut renderer::RenderingContext = self.ctx;
        ctx.set_clear_color(0.8, 0.8, 0.8, 1.0);

        let cube_vertices: &[f32] = &[
          // Front face     |     normals     | tex coords
          -1.0, -1.0, 1.0,    0.0, 0.0, 1.0,    1.0, 0.0,
           1.0, -1.0, 1.0,    0.0, 0.0, 1.0,    1.0, 1.0,
           1.0,  1.0, 1.0,    0.0, 0.0, 1.0,    0.0, 1.0,
          -1.0,  1.0, 1.0,    0.0, 0.0, 1.0,    0.0, 0.0,
          // Back face
          -1.0, -1.0, -1.0,   0.0, 0.0, -1.0,   1.0, 0.0,
          -1.0,  1.0, -1.0,   0.0, 0.0, -1.0,   1.0, 1.0,
           1.0,  1.0, -1.0,   0.0, 0.0, -1.0,   0.0, 1.0,
           1.0, -1.0, -1.0,   0.0, 0.0, -1.0,   0.0, 0.0,
          // Top face
          -1.0, 1.0, -1.0,    0.0, 1.0, 1.0,    1.0, 0.0,
          -1.0, 1.0,  1.0,    0.0, 1.0, 1.0,    1.0, 1.0,
           1.0, 1.0,  1.0,    0.0, 1.0, 1.0,    0.0, 1.0,
           1.0, 1.0, -1.0,    0.0, 1.0, 1.0,    0.0, 0.0,
          // Bottom face
          -1.0, -1.0, -1.0,   0.0, -1.0, 1.0,   1.0, 0.0,
           1.0, -1.0, -1.0,   0.0, -1.0, 1.0,   1.0, 1.0,
           1.0, -1.0,  1.0,   0.0, -1.0, 1.0,   0.0, 1.0,
          -1.0, -1.0,  1.0,   0.0, -1.0, 1.0,   0.0, 0.0,
          // Right face
           1.0, -1.0, -1.0,   1.0, 0.0, 1.0,    1.0, 0.0,
           1.0,  1.0, -1.0,   1.0, 0.0, 1.0,    1.0, 1.0,
           1.0,  1.0,  1.0,   1.0, 0.0, 1.0,    0.0, 1.0,
           1.0, -1.0,  1.0,   1.0, 0.0, 1.0,    0.0, 0.0,
          // Left face
          -1.0, -1.0, -1.0,   -1.0, 0.0, 1.0,   1.0, 0.0,
          -1.0, -1.0,  1.0,   -1.0, 0.0, 1.0,   1.0, 1.0,
          -1.0,  1.0,  1.0,   -1.0, 0.0, 1.0,   0.0, 1.0,
          -1.0,  1.0, -1.0,   -1.0, 0.0, 1.0,   0.0, 0.0
        ];

        let cube_indices: &[u16] = &[
          0, 1, 2, 0, 2, 3,         // Front face
          4, 5, 6, 4, 6, 7,         // Back face
          8, 9, 10, 8, 10, 11,      // Top face
          12, 13, 14, 12, 14, 15,   // Bottom face
          16, 17, 18, 16, 18, 19,   // Right face
          20, 21, 22, 20, 22, 23    // Left face
        ];

        let cube_vbo = ctx.create_buffer();
        let cube_ibo = ctx.create_buffer();

        ctx.upload_buffer(
            cube_vbo,
            renderer::VERTEX_BUFFER,
            renderer::STATIC,
            renderer::as_bytes(cube_vertices)
        ).ok().expect("cube vbo upload");
        ctx.upload_buffer(
            cube_ibo,
            renderer::INDEX_BUFFER,
            renderer::STATIC,
            renderer::as_bytes(cube_indices)
        ).ok().expect("cube ibo upload");

        let cube_geom = ctx.create_geometry([
            renderer::VertexAttribute {
                buffer: cube_vbo,
                attrib_type: renderer::F32,
                components: 3,
                location: a_position,
                stride: 32,
                offset: 0,
                normalize: false,
            },
            renderer::VertexAttribute {
                buffer: cube_vbo,
                attrib_type: renderer::F32,
                components: 3,
                location: a_normals,
                stride: 32,
                offset: 12,
                normalize: false,
            },
            renderer::VertexAttribute {
                buffer: cube_vbo,
                attrib_type: renderer::F32,
                components: 2,
                location: a_tex_coords,
                stride: 32,
                offset: 24,
                normalize: false,
            }],
            Some(cube_ibo)
        ).ok().expect("cube geom definition");

        let quad_vertices: &[f32] = &[
              0.0,   0.0,   0.0, 0.0,
            200.0,   0.0,   1.0, 0.0,
            200.0, 200.0,   1.0, 1.0,
              0.0, 200.0,   0.0, 1.0,
        ];
        let quad_indices: &[u16] = &[0, 1, 2, 0, 2, 3];

        let quad_vbo = ctx.create_buffer();
        let quad_ibo = ctx.create_buffer();

        ctx.upload_buffer(
            quad_vbo,
            renderer::VERTEX_BUFFER,
            renderer::STATIC,
            renderer::as_bytes(quad_vertices)
        ).ok().expect("vbo upload");

        ctx.upload_buffer(
            quad_ibo,
            renderer::INDEX_BUFFER,
            renderer::STATIC,
            renderer::as_bytes(quad_indices)
        ).ok().expect("ibo upload");

        let geom = ctx.create_geometry([
            renderer::VertexAttribute {
                buffer: quad_vbo,
                attrib_type: renderer::F32,
                components: 2,
                location: a_position,
                stride: 16,
                offset: 0,
                normalize: false,
            },
            renderer::VertexAttribute {
                buffer: quad_vbo,
                attrib_type: renderer::F32,
                components: 2,
                location: a_tex_coords,
                stride: 16,
                offset: 8,
                normalize: false,
            }],
            Some(quad_ibo)
        ).ok().expect("geom creation");

        let text = "vodk! - Hello World";
        let mut text_vertices = Vec::from_fn(
            text.len()*24,
            |_|{ 0.0 as f32 }
        );
        text::text_buffer(text, 0.0, -0.5, 0.04, 0.08, text_vertices.as_mut_slice());
        let text_vbo = ctx.create_buffer();
        ctx.upload_buffer(
            text_vbo,
            renderer::VERTEX_BUFFER,
            renderer::STATIC,
            renderer::as_bytes(text_vertices.as_slice())
        ).ok().expect("text vbo upload");

        let text_geom = ctx.create_geometry([
            renderer::VertexAttribute {
                buffer: text_vbo,
                attrib_type: renderer::F32,
                components: 2,
                location: a_position,
                stride: 4*4,
                offset: 0,
                normalize: false,
            },
            renderer::VertexAttribute {
                buffer: text_vbo,
                attrib_type: renderer::F32,
                components: 2,
                location: a_tex_coords,
                stride: 4*4,
                offset: 2*4,
                normalize: false,
            }],
            None
        ).ok().expect("text geom creation");

        let (text_program, text_uniforms) = setup_shader(ctx,
            shaders::TEXT_VERTEX_SHADER,
            shaders::TEXT_FRAGMENT_SHADER
        );

        let (program_3d, uniforms_3d) = setup_shader(ctx,
            shaders::BASIC_VERTEX_SHADER_3D,
            shaders::NORMALS_FRAGMENT_SHADER
        );

        let (program_tex_2d, uniforms_2d) = setup_shader(ctx,
            shaders::BASIC_VERTEX_SHADER_2D,
            shaders::TEXTURED_FRAGMENT_SHADER
        );

        let ascii_atlas = match png::load_png(&Path::new("assets/ascii_atlas.png")) {
            Ok(img) => img,
            Err(e) => fail!("Failed to load the ascii atlas image {}", e)
        };

        let ascii_tex = ctx.create_texture(renderer::REPEAT|renderer::FILTER_LINEAR);
        ctx.upload_texture_data(
            ascii_tex, ascii_atlas.pixels.as_slice(),
            ascii_atlas.width, ascii_atlas.height,
            renderer::R8G8B8A8
        );

        let checker = create_checker_texture(10, 10, ctx);
        let screen = ctx.get_default_render_target();

        self.shaders.push((
            program_tex_2d,
            UniformLayout::new(ctx, program_tex_2d)
        ));
        self.draw_calls.push((
            renderer::COLOR,
            renderer::GeometryRange {
                geometry: geom,
                from: 0, to: 6,
                flags: renderer::TRIANGLES
            }
        ));

        self.shaders.push((
            text_program,
            UniformLayout::new(ctx, text_program)
        ));
        self.draw_calls.push((
            renderer::COLOR,
            renderer::GeometryRange {
                geometry: text_geom,
                from: 0, to: (text.len()*6) as u32,
                flags: renderer::TRIANGLES
            }
        ));

        self.shaders.push((
            program_3d,
            UniformLayout::new(ctx, program_3d)
        ));
        self.draw_calls.push((
            renderer::COLOR|renderer::DEPTH,
            renderer::GeometryRange {
                geometry: cube_geom,
                from: 0,
                to: cube_indices.len() as u32,
                flags: renderer::TRIANGLES
            }
        ));

        self.textures.push(checker);
        self.textures.push(ascii_tex);
    }
}

static a_position:   renderer::VertexAttributeLocation = 0;
static a_normals:    renderer::VertexAttributeLocation = 1;
static a_tex_coords: renderer::VertexAttributeLocation = 2;

#[deriving(Show)]
struct UniformLayout {
    pub u_resolution: renderer::ShaderInputLocation,
    pub u_color: renderer::ShaderInputLocation,
    pub u_texture_0: renderer::ShaderInputLocation,
    pub u_texture_1: renderer::ShaderInputLocation,
    pub u_texture_2: renderer::ShaderInputLocation,
    pub u_texture_3: renderer::ShaderInputLocation,
    pub u_model_mat: renderer::ShaderInputLocation,
    pub u_view_mat: renderer::ShaderInputLocation,
    pub u_proj_mat: renderer::ShaderInputLocation,
}

impl UniformLayout {
    fn new(ctx: &mut renderer::RenderingContext, p: renderer::ShaderProgram) -> UniformLayout{
        return UniformLayout {
            u_color: ctx.get_shader_input_location(p, "u_color"),
            u_texture_0: ctx.get_shader_input_location(p, "u_texture_0"),
            u_texture_1: ctx.get_shader_input_location(p, "u_texture_1"),
            u_texture_2: ctx.get_shader_input_location(p, "u_texture_2"),
            u_texture_3: ctx.get_shader_input_location(p, "u_texture_3"),
            u_model_mat: ctx.get_shader_input_location(p, "u_model_mat"),
            u_view_mat:  ctx.get_shader_input_location(p, "u_view_mat"),
            u_proj_mat:  ctx.get_shader_input_location(p, "u_proj_mat"),
            u_resolution: ctx.get_shader_input_location(p, "u_resolution"),
        }
    }
}

fn setup_shader(
    ctx: &mut renderer::RenderingContext,
    vs_src: &str,
    fs_src: &str
) -> (renderer::ShaderProgram, UniformLayout) {
    let vs = ctx.create_shader(renderer::VERTEX_SHADER);
    let fs = ctx.create_shader(renderer::FRAGMENT_SHADER);
    let program = ctx.create_shader_program();

    ctx.compile_shader(vs, &[vs_src]).map_err(
        |e| { fail!("Failed to compile the vertex shader: {}", e); return; }
    );

    ctx.compile_shader(fs, &[fs_src]).map_err(
        |e| { fail!("Failed to compile the fragment shader: {}", e); return; }
    );

    ctx.link_shader_program(program, [vs, fs], &[
        ("a_position", a_position),
        ("a_normals", a_normals),
        ("a_tex_coords", a_tex_coords)
    ]).map_err(
        |e| { fail!("Failed to link the text's shader program: {}", e); return; }
    );

    let uniforms = UniformLayout::new(ctx, program);
    ctx.destroy_shader(vs);
    ctx.destroy_shader(fs);
    return (program, uniforms);
}

fn quad(
    x: f32, y: f32, w: f32, h: f32,
    vertices: &mut [f32], indices: &mut [u16],
    vertex_stride: uint, vertex_offset: u16
) {
    vertices[0] = x;
    vertices[1] = y;
    vertices[vertex_stride + 0] = x;
    vertices[vertex_stride + 1] = y + h;
    vertices[vertex_stride * 2 + 0] = x + w;
    vertices[vertex_stride * 2 + 1] = y + h;
    vertices[vertex_stride * 3 + 0] = x + w;
    vertices[vertex_stride * 3 + 1] = y;
    indices[0] = vertex_offset;
    indices[1] = vertex_offset + 1;
    indices[2] = vertex_offset + 2;
    indices[3] = vertex_offset;
    indices[4] = vertex_offset + 2;
    indices[5] = vertex_offset + 3;
}

fn main() {
    std::io::println("vodk!");

    let mut window = gfx::window::Window::create(800, 600, "vodk");
    let mut ctx = window.create_rendering_context();
    let mut app = TestApp::new(&mut window, ctx);
    app.init();
    let app = &mut app as &mut app::Application;

    let mut avg_frame_time: u64 = 0;
    let mut frame_count: u64 = 0;
    let mut previous_time = time::precise_time_ns();
    let mut i = 0;
    while !window.should_close() {
        window.poll_events();
        let frame_start_time = time::precise_time_ns();
        let elapsed_time = frame_start_time - previous_time;

        app.update(elapsed_time as f32 / 1000000.0 , i);

        i+=1;
        previous_time = frame_start_time;
        let frame_time = time::precise_time_ns() - frame_start_time;
        frame_count += 1;
        avg_frame_time += frame_time;

        if frame_count % 60 == 0 {
            println!("avg frame time: {}ms", avg_frame_time as f64/(60.0*1000000.0));
            avg_frame_time = 0;
        }

        let sleep_time: i64 = 16000000 - frame_time as i64;
        if sleep_time > 0 {
            sleep(sleep_time as u64/1000000 );
        }
    }

    app.shut_down();
}

fn create_checker_texture(w: uint, h: uint, ctx: &mut renderer::RenderingContext) -> renderer::Texture {
    let checker_data: Vec<u8> = Vec::from_fn(w*h*4, |i|{
        (((i / 4 + (i/(4*h))) % 2)*255) as u8
    });
    let checker = ctx.create_texture(renderer::REPEAT|renderer::FILTER_NEAREST);
    ctx.upload_texture_data(checker, checker_data.as_slice(),
        w as u32, h as u32, renderer::R8G8B8A8);
    return checker;
}
