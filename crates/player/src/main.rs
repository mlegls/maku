//! The debug player: a sim+render SERVER (sclang/scsynth split).
//!
//! Usage: maku [card.maku [pattern-name]]
//! With no card argument the player starts empty and waits for clients.
//!
//! The CLI card argument is the degenerate client. A TCP listener on
//! 127.0.0.1:7777 accepts newline-delimited EDN commands — the wire format
//! is the card format — so editor clients (vim plugin) are thin
//! send-form-to-socket shims:
//!
//!   (run <forms...>)                 run forms as an anonymous pattern
//!                                     (current card's defs in scope; the
//!                                     input tape replays through new code)
//!   (swap <forms...>)                generational hot-swap: in-flight
//!                                     bullets keep their old code
//!   (add <forms...>)                 layer onto the running sim; the added
//!                                     pattern's clocks anchor at this tick
//!   (load "path/to/card.maku")        reload from disk (does NOT play)
//!   (load "path" "pattern-name")     reload and play the named pattern
//!   (pattern "name")                 switch pattern in the current card
//!   (restart)                        re-run the current pattern
//!   (clear)                          stop the running pattern
//!   (seek N) (step ±N)               scrub the timeline (pauses). The sim
//!                                    is a deterministic fold over two tapes
//!                                    (inputs + program commands); backward =
//!                                    snapshot + re-step, and add/swap
//!                                    boundaries replay at their ticks
//!   (snapshots N)                    snapshot cadence in ticks; 0 = off
//!                                    (old snapshots auto-thin regardless)
//!   (resize-entities N)              explicit host-side entity capacity
//!                                    change, recorded on the command tape
//!   (pause) (resume)

use maku::host::Instance;
use maku::interp::Val;
use maku::sim::Inputs;
use maku_mesh_touhou::{
    AddressMode, BlendMode, DrawSource, MaterialDesc, MaterialId, MeshFrame, TextureFilter,
    TextureSource, TouhouMesh, TouhouProfile,
};
use macroquad::miniquad::{Backend, BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
use macroquad::prelude::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};

const PORT: u16 = 7777;
const PIXELS_PER_UNIT: f32 = 55.0;
const TIMELINE_H: f32 = 30.0;

/// Forward raw command lines; Forms are Rc-based (interpreter-local), so
/// parsing happens on the sim thread.
fn serve(port: u16) -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("server: bind {}: {}", port, e);
                return;
            }
        };
        eprintln!("server: listening on 127.0.0.1:{}", port);
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stream);
                for line in reader.lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        let _ = tx.send(line);
                    }
                }
            });
        }
    });
    rx
}

/// Macroquad compatibility adapter. The pack stays instanced; this host alone
/// expands sprite commands and remaps host-neutral u32 strip indices to u16.
struct RenderResources {
    textures: Vec<Texture2D>,
    materials: Vec<MaterialDesc>,
    pipelines: Vec<Material>,
}

impl RenderResources {
    fn resolve(profile: &TouhouProfile) -> Result<Self, String> {
        let mut textures = Vec::with_capacity(profile.textures().len());
        for texture in profile.textures() {
            let resolved = match &texture.source {
                TextureSource::BuiltinRgba8 { width, height, bytes } => {
                    let width = u16::try_from(*width).map_err(|_| format!("texture '{}' is too wide", texture.key))?;
                    let height = u16::try_from(*height).map_err(|_| format!("texture '{}' is too high", texture.key))?;
                    Texture2D::from_rgba8(width, height, bytes)
                }
                TextureSource::External { key } => {
                    return Err(format!("external texture '{}' is not registered in the debug player", key));
                }
            };
            textures.push(resolved);
        }
        let mut pipelines = Vec::with_capacity(profile.materials().len());
        let mut filters = vec![None; textures.len()];
        for material in profile.materials() {
            if material.texture.0 as usize >= textures.len() {
                return Err(format!("material '{}' has no resolved texture", material.key));
            }
            if !matches!(material.pipeline.as_ref(), "touhou.sprite.v1" | "touhou.ribbon.v1") {
                return Err(format!("debug player has no registered pipeline '{}'", material.pipeline));
            }
            if material.sampler.address_u != AddressMode::Clamp || material.sampler.address_v != AddressMode::Clamp {
                return Err(format!("debug player material '{}' requires unsupported non-clamp addressing", material.key));
            }
            if material.sampler.min_filter != material.sampler.mag_filter {
                return Err(format!("debug player material '{}' requires distinct min/mag filters", material.key));
            }
            let filter = match material.sampler.min_filter {
                TextureFilter::Nearest => FilterMode::Nearest,
                TextureFilter::Linear => FilterMode::Linear,
            };
            let slot = &mut filters[material.texture.0 as usize];
            if slot.is_some_and(|existing| existing != filter) {
                return Err(format!("texture {} is referenced with conflicting filters", material.texture.0));
            }
            *slot = Some(filter);
            pipelines.push(resolve_pipeline(material)?);
        }
        for (texture, filter) in textures.iter().zip(filters) {
            texture.set_filter(filter.unwrap_or(FilterMode::Linear));
        }
        Ok(Self { textures, materials: profile.materials().to_vec(), pipelines })
    }

    fn material(&self, id: MaterialId) -> (&MaterialDesc, &Texture2D, &Material) {
        let index = id.0 as usize;
        let material = &self.materials[index];
        (material, &self.textures[material.texture.0 as usize], &self.pipelines[index])
    }
}

fn resolve_pipeline(desc: &MaterialDesc) -> Result<Material, String> {
    let blend = match desc.blend {
        BlendMode::Opaque => None,
        BlendMode::Alpha => Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        )),
        BlendMode::Additive => Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::One,
        )),
        BlendMode::SoftAdditive => Some(BlendState::new(
            Equation::Add,
            BlendFactor::OneMinusValue(BlendValue::DestinationColor),
            BlendFactor::One,
        )),
    };
    let params = MaterialParams {
        pipeline_params: PipelineParams { color_blend: blend, alpha_blend: blend, ..Default::default() },
        ..Default::default()
    };
    let recolor = desc.layout == maku_mesh_touhou::SourceLayout::RecolorSprite;
    let backend = unsafe { get_internal_gl().quad_context.info().backend };
    let shader = match backend {
        Backend::OpenGl => ShaderSource::Glsl {
            vertex: SPRITE_VERTEX_GLSL,
            fragment: if recolor { RECOLOR_FRAGMENT_GLSL } else { STANDARD_FRAGMENT_GLSL },
        },
        Backend::Metal => ShaderSource::Msl {
            program: if recolor { RECOLOR_MSL } else { STANDARD_MSL },
        },
    };
    load_material(shader, params).map_err(|error| format!("material '{}': {error}", desc.key))
}

const SPRITE_VERTEX_GLSL: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying lowp vec2 uv;
varying lowp vec4 color;
varying lowp vec4 recolor_high;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    uv = texcoord;
    color = color0 / 255.0;
    recolor_high = normal;
}"#;
const STANDARD_FRAGMENT_GLSL: &str = r#"#version 100
varying lowp vec2 uv;
varying lowp vec4 color;
uniform sampler2D Texture;
void main() { gl_FragColor = color * texture2D(Texture, uv); }"#;
const RECOLOR_FRAGMENT_GLSL: &str = r#"#version 100
varying lowp vec2 uv;
varying lowp vec4 color;
varying lowp vec4 recolor_high;
uniform sampler2D Texture;
void main() {
    lowp vec4 sample = texture2D(Texture, uv);
    gl_FragColor = mix(color, recolor_high, sample.r) * sample.a;
}"#;
const STANDARD_MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Uniforms { float4x4 Model; float4x4 Projection; };
struct Vertex { float3 position [[attribute(0)]]; float2 uv [[attribute(1)]]; float4 color [[attribute(2)]]; float4 normal [[attribute(3)]]; };
struct Raster { float4 position [[position]]; float2 uv [[user(locn0)]]; float4 color [[user(locn1)]]; float4 high [[user(locn2)]]; };
vertex Raster vertexShader(Vertex v [[stage_in]], constant Uniforms& u [[buffer(0)]]) {
    Raster o; o.position = u.Projection * u.Model * float4(v.position, 1); o.uv = v.uv; o.color = v.color / 255.0; o.high = v.normal; return o;
}
fragment float4 fragmentShader(Raster in [[stage_in]], texture2d<float> tex [[texture(0)]], sampler smp [[sampler(0)]]) {
    return in.color * tex.sample(smp, in.uv);
}"#;
const RECOLOR_MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Uniforms { float4x4 Model; float4x4 Projection; };
struct Vertex { float3 position [[attribute(0)]]; float2 uv [[attribute(1)]]; float4 color [[attribute(2)]]; float4 normal [[attribute(3)]]; };
struct Raster { float4 position [[position]]; float2 uv [[user(locn0)]]; float4 color [[user(locn1)]]; float4 high [[user(locn2)]]; };
vertex Raster vertexShader(Vertex v [[stage_in]], constant Uniforms& u [[buffer(0)]]) {
    Raster o; o.position = u.Projection * u.Model * float4(v.position, 1); o.uv = v.uv; o.color = v.color / 255.0; o.high = v.normal; return o;
}
fragment float4 fragmentShader(Raster in [[stage_in]], texture2d<float> tex [[texture(0)]], sampler smp [[sampler(0)]]) {
    float4 sample = tex.sample(smp, in.uv); return mix(in.color, in.high, sample.r) * sample.a;
}"#;

fn draw_frame(frame: &MeshFrame, resources: &RenderResources, cx: f32, cy: f32) {
    for command in &frame.draws {
        let (material, texture, pipeline) = resources.material(command.material);
        debug_assert_eq!(material.layout, command.source.layout());
        gl_use_material(pipeline);
        match command.source {
            DrawSource::BasicSprites { start, count } => {
                let instances = &frame.basic_sprites[start as usize..(start + count) as usize];
                let fixed = material.fixed_color.expect("validated basic material").0;
                draw_sprite_instances(instances.iter().map(|v| {
                    let alpha = ((fixed[3] as u16 * v.alpha as u16 + 127) / 255) as u8;
                    (v, [fixed[0], fixed[1], fixed[2], alpha], [0.0; 4])
                }), texture, cx, cy);
            }
            DrawSource::TintedSprites { start, count } => {
                let instances = &frame.tinted_sprites[start as usize..(start + count) as usize];
                draw_sprite_instances(instances.iter().map(|v| (&v.base, v.tint, [0.0; 4])), texture, cx, cy);
            }
            DrawSource::RecolorSprites { start, count } => {
                let instances = &frame.recolor_sprites[start as usize..(start + count) as usize];
                draw_sprite_instances(instances.iter().map(|v| {
                    let high = v.color_hi.map(|c| c as f32 / 255.0);
                    (&v.base, v.color_lo, high)
                }), texture, cx, cy);
            }
            DrawSource::Indexed { index_start, index_count, .. } => {
                draw_indexed(frame, index_start, index_count, texture, cx, cy);
            }
        }
        gl_use_default_material();
    }
}

fn draw_sprite_instances<'a>(
    instances: impl Iterator<Item = (&'a maku_mesh_touhou::BasicSpriteInstance, [u8; 4], [f32; 4])>,
    texture: &Texture2D,
    cx: f32,
    cy: f32,
) {
    const QUADS_PER_CHUNK: usize = u16::MAX as usize / 4;
    let mut vertices = Vec::with_capacity(QUADS_PER_CHUNK * 4);
    let mut indices = Vec::with_capacity(QUADS_PER_CHUNK * 6);
    for (instance, color, recolor_high) in instances {
        if vertices.len() + 4 > u16::MAX as usize {
            draw_mesh(&Mesh { vertices, indices, texture: Some(texture.clone()) });
            vertices = Vec::with_capacity(QUADS_PER_CHUNK * 4);
            indices = Vec::with_capacity(QUADS_PER_CHUNK * 6);
        }
        let base = vertices.len() as u16;
        let angle = instance.rotation.to_radians();
        let (s, c) = angle.sin_cos();
        let [u0, v0, u1, v1] = instance.uv_rect;
        for ([lx, ly], uv) in [
            ([-instance.half_size[0], -instance.half_size[1]], [u0, v0]),
            ([ instance.half_size[0], -instance.half_size[1]], [u1, v0]),
            ([ instance.half_size[0],  instance.half_size[1]], [u1, v1]),
            ([-instance.half_size[0],  instance.half_size[1]], [u0, v1]),
        ] {
            let wx = instance.center[0] + c * lx - s * ly;
            let wy = instance.center[1] + s * lx + c * ly;
            vertices.push(macroquad::models::Vertex {
                position: vec3(cx + wx * PIXELS_PER_UNIT, cy - wy * PIXELS_PER_UNIT, 0.0),
                uv: vec2(uv[0], uv[1]), color,
                normal: vec4(recolor_high[0], recolor_high[1], recolor_high[2], recolor_high[3]),
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    if !indices.is_empty() { draw_mesh(&Mesh { vertices, indices, texture: Some(texture.clone()) }); }
}

fn draw_indexed(frame: &MeshFrame, index_start: u32, index_count: u32, texture: &Texture2D, cx: f32, cy: f32) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut remap = HashMap::<u32, u16>::new();
    let source = &frame.indices[index_start as usize..(index_start + index_count) as usize];
    for tri in source.chunks_exact(3) {
        let fresh = tri.iter().filter(|i| !remap.contains_key(i)).count();
        if !indices.is_empty() && vertices.len() + fresh > u16::MAX as usize {
            draw_mesh(&Mesh { vertices, indices, texture: Some(texture.clone()) });
            vertices = Vec::new(); indices = Vec::new(); remap.clear();
        }
        for source_index in tri {
            let local = *remap.entry(*source_index).or_insert_with(|| {
                let v = frame.vertices[*source_index as usize];
                let local = vertices.len() as u16;
                vertices.push(macroquad::models::Vertex {
                    position: vec3(cx + v.pos[0] * PIXELS_PER_UNIT, cy - v.pos[1] * PIXELS_PER_UNIT, 0.0),
                    uv: vec2(v.uv[0], v.uv[1]), color: v.color,
                    normal: vec4(0.0, 0.0, 0.0, 0.0),
                });
                local
            });
            indices.push(local);
        }
    }
    if !indices.is_empty() { draw_mesh(&Mesh { vertices, indices, texture: Some(texture.clone()) }); }
}

/// Bottom strip: play/pause button + timeline slider over the recorded tape.
/// Dragging the handle scrubs (auto-pauses); clicking play resumes, which
/// branches the timeline (truncates the future) like every other resume.
fn timeline_ui(app: &mut App, mx: f32, my: f32) {
    let Some(tl) = app.inst.timeline() else { return };
    let cur = tl.tick;
    let h = screen_height();
    let w = screen_width();
    let cy = h - TIMELINE_H / 2.0;
    let total = tl.tape_len.max(1) as f32;

    // play/pause button
    let (bx, br) = (22.0, 9.0);
    let over_btn = (mx - bx).abs() < 14.0 && (my - cy).abs() < 14.0;
    let btn_col = if over_btn { WHITE } else { GRAY };
    if app.inst.paused() {
        draw_triangle(
            Vec2::new(bx - br * 0.6, cy - br),
            Vec2::new(bx - br * 0.6, cy + br),
            Vec2::new(bx + br, cy),
            btn_col,
        );
    } else {
        draw_rectangle(bx - br * 0.7, cy - br, br * 0.55, br * 2.0, btn_col);
        draw_rectangle(bx + br * 0.15, cy - br, br * 0.55, br * 2.0, btn_col);
    }
    if over_btn && is_mouse_button_pressed(MouseButton::Left) {
        app.inst.toggle_pause();
        return;
    }

    // slider track
    let (x0, x1) = (44.0, w - 96.0);
    let frac = (cur as f32 / total).clamp(0.0, 1.0);
    let hx = x0 + frac * (x1 - x0);
    draw_line(x0, cy, x1, cy, 3.0, Color::new(1.0, 1.0, 1.0, 0.15));
    draw_line(x0, cy, hx, cy, 3.0, Color::new(0.4, 0.7, 1.0, 0.8));
    // snapshot notches
    for st in &tl.snap_ticks {
        let sx = x0 + (*st as f32 / total).clamp(0.0, 1.0) * (x1 - x0);
        draw_line(sx, cy - 4.0, sx, cy + 4.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.2));
    }
    // command-tape markers: where adds/swaps landed
    for ct in &tl.cmd_ticks {
        let sx = x0 + (*ct as f32 / total).clamp(0.0, 1.0) * (x1 - x0);
        draw_line(sx, cy - 6.0, sx, cy + 6.0, 2.0, Color::new(1.0, 0.7, 0.3, 0.8));
    }
    draw_circle(hx, cy, 6.0, if app.dragging { WHITE } else { LIGHTGRAY });
    draw_text(
        &format!("{} / {}", cur, tl.tape_len),
        x1 + 10.0,
        cy + 5.0,
        16.0,
        GRAY,
    );

    // drag to scrub
    let over_track = mx >= x0 - 8.0 && mx <= x1 + 8.0 && (my - cy).abs() < 12.0;
    if over_track && is_mouse_button_pressed(MouseButton::Left) {
        app.dragging = true;
    }
    if !is_mouse_button_down(MouseButton::Left) {
        app.dragging = false;
    }
    if app.dragging {
        let f = ((mx - x0) / (x1 - x0)).clamp(0.0, 1.0);
        let target = (f * tl.tape_len as f32).round() as u64;
        if target != cur {
            app.inst.seek(target);
        }
    }
}

/// Host-local per-frame state around the core Instance.
mod bindings;
use bindings::Bindings;

struct App {
    inst: Instance,
    accum: f64,
    dragging: bool,   // scrubbing via the timeline slider
    binds: Bindings,  // key→channel bindings + constant channels (B panel)
}

fn window_conf() -> Conf {
    Conf {
        window_title: "maku".into(),
        window_width: 900,
        window_height: 960,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    // usage: maku [--rig] [card.maku [pattern-name]] — with no card,
    // start empty and wait for (load ...) / (run ...) from clients
    let (flags, plain): (Vec<String>, Vec<String>) =
        std::env::args().skip(1).partition(|a| a.starts_with("--"));
    let card_path = plain.first().cloned().unwrap_or_default();
    let pattern = plain.get(1).cloned();

    // this host's player contract: the MOUSE MOCK by default — $player
    // rides the cursor, which is what tutorial/demo cards want. Cards
    // that want a piloted rig spawn one in card code (reimu_vs_mima
    // spawns `(player ...)`; `(player-rig)` is the stock invocation),
    // or pass --rig to layer the stock arrow rig over any card.
    let rig = if flags.iter().any(|f| f == "--rig") {
        // expand_src: the rig shim imports its defs ((import "touhou")),
        // and raw source would hand the sim a literal import form
        Some(
            maku::edn::expand_src(&format!(
                "{}\n(player-rig)",
                maku::edn::stdlib("player-rig").unwrap()
            ))
            .expect("player-rig import expansion"),
        )
    } else {
        None
    };
    let mut app = App {
        inst: Instance::new(rig),
        accum: 0.0,
        dragging: false,
        binds: Bindings::defaults(),
    };
    let mut provided = app.binds.channel_names();
    for m in ["player", "nearest-enemy"] {
        if !provided.iter().any(|n| n == m) { provided.push(m.to_string()); }
    }
    app.inst.host_channels = Some(provided);
    app.inst.render_kinds = Some(TouhouMesh::RENDER_KINDS.iter().map(|k| (*k).into()).collect());
    if card_path.is_empty() {
        app.inst.set_status(format!("no card — listening on 127.0.0.1:{}", PORT));
    } else {
        app.inst.boot(card_path, pattern);
    }
    let commands = serve(PORT);
    let profile = std::rc::Rc::new(TouhouProfile::stock());
    let resources = RenderResources::resolve(&profile).expect("resolve Touhou render resources");
    let mut touhou = TouhouMesh::new(profile);

    loop {
        // server commands
        while let Ok(line) = commands.try_recv() {
            app.inst.command_line(&line);
        }
        // hotkeys: r = restart from disk, c = clear, space = pause, esc = quit
        let panel_open = app.binds.wants_keys();
        if !panel_open && is_key_pressed(KeyCode::R) {
            app.inst.reload_restart();
        }
        if !panel_open && is_key_pressed(KeyCode::C) {
            app.inst.clear();
        }
        if !panel_open && is_key_pressed(KeyCode::Space) {
            app.inst.toggle_pause();
        }
        // scrub hotkeys — only while paused (live arrows belong to movement)
        if app.inst.paused() {
            if let Some(cur) = app.inst.tick() {
                if is_key_pressed(KeyCode::Right) {
                    app.inst.seek(cur + 1);
                }
                if is_key_pressed(KeyCode::Left) {
                    app.inst.seek(cur.saturating_sub(1));
                }
                if is_key_pressed(KeyCode::Up) {
                    app.inst.seek(cur + 30);
                }
                if is_key_pressed(KeyCode::Down) {
                    app.inst.seek(cur.saturating_sub(30));
                }
            }
        }
        // pattern menu: 1-9 select a defpattern from the card
        for (i, key) in [
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
            KeyCode::Key6,
            KeyCode::Key7,
            KeyCode::Key8,
            KeyCode::Key9,
        ]
        .iter()
        .enumerate()
        {
            if !panel_open && is_key_pressed(*key) {
                app.inst.select(i);
            }
        }
        // difficulty: T/Y/U/I quick-set the $rank constant (easy..lunatic)
        if !panel_open {
            for (key, r) in [
                (KeyCode::T, 0.7),
                (KeyCode::Y, 1.0),
                (KeyCode::U, 1.4),
                (KeyCode::I, 2.0),
            ] {
                if is_key_pressed(key) {
                    app.binds.set_const("rank", r);
                }
            }
        }
        // Tab / Shift-Tab step through ALL patterns (hotkeys stop at 9)
        if !panel_open && is_key_pressed(KeyCode::Tab) {
            let names = app.inst.patterns().to_vec();
            if !names.is_empty() {
                let cur = app.inst.current_pattern().unwrap_or_default();
                let at = names.iter().position(|n| *n == cur);
                let next = if is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift) {
                    at.map(|i| (i + names.len() - 1) % names.len()).unwrap_or(names.len() - 1)
                } else {
                    at.map(|i| (i + 1) % names.len()).unwrap_or(0)
                };
                app.inst.select(next);
            }
        }
        if !panel_open && is_key_pressed(KeyCode::Escape) {
            break;
        }

        // mock player rides the mouse (design.md §11: sandbox mock player)
        let (cx, cy) = (screen_width() / 2.0, screen_height() / 2.0 + 100.0);
        let (mx, my) = mouse_position();
        let mouse_world = (
            ((mx - cx) / PIXELS_PER_UNIT) as f64,
            ((cy - my) / PIXELS_PER_UNIT) as f64,
        );
        // raw input side of the host contract: the BINDING TABLE (B panel)
        // maps keys to channels — axes for movement, buttons with
        // tap/hold/toggle modes, plus constant channels ($rank lives
        // there). The mouse remains the mock $player / $nearest-enemy.
        // Replays are unaffected: the tape records channel VALUES.
        let mut inputs = Inputs::classic(mouse_world, mouse_world);
        if !panel_open {
            app.binds.poll_edges();
        }
        app.binds.inject(&mut inputs, app.inst.paused());
        // this host's side of the load-time manifest check: binding-panel
        // channels plus the mouse mocks
        let mut provided = app.binds.channel_names();
        for m in ["player", "nearest-enemy"] {
            if !provided.iter().any(|n| n == m) {
                provided.push(m.to_string());
            }
        }
        app.inst.host_channels = Some(provided);

        // fixed-timestep sim (design.md §4: variable dt never reaches the sim)
        if !app.inst.paused() {
            app.accum += get_frame_time() as f64;
            let dt = 1.0 / app.inst.tick_rate();
            while app.accum >= dt {
                app.accum -= dt;
                app.inst.advance(inputs.clone());
                // taps are high for exactly one stepped tick
                app.binds.consume_taps(&mut inputs);
                if app.inst.paused() {
                    break; // sim error pauses
                }
            }
        }

        clear_background(Color::from_rgba(0x12, 0x12, 0x1a, 0xff));
        if app.inst.running() {
            let mut schema_error = None;
            for kind in ["sprite", "beam"] {
                if let Some(schema) = app.inst.declared_render_schema(kind) {
                    if let Err(error) = touhou.bind_schema(kind, schema) {
                        schema_error = Some(error);
                        break;
                    }
                }
            }
            match schema_error {
                Some(error) => app.inst.set_status(format!("render schema error: {error}")),
                None => {
                    let frame = app.inst.render_frame();
                    match touhou.build(&frame) {
                        Ok(mesh) => draw_frame(mesh, &resources, cx, cy),
                        Err(error) => app.inst.set_status(format!("render error: {error}")),
                    }
                }
            }
        }
        // player marker at the $player channel (derived from a piloted rig,
        // or the mouse): true hitbox dot + graze ring
        let (pmx, pmy) = match app.inst.channel("player") {
            Some(Val::Pose(p)) => {
                (cx + p.x as f32 * PIXELS_PER_UNIT, cy - p.y as f32 * PIXELS_PER_UNIT)
            }
            _ => (mx, my),
        };
        draw_circle_lines(pmx, pmy, 0.35 * PIXELS_PER_UNIT, 1.0, Color::new(1.0, 1.0, 1.0, 0.25));
        draw_circle_lines(pmx, pmy, 8.0, 2.0, Color::new(1.0, 1.0, 1.0, 0.8));
        draw_circle(pmx, pmy, 0.06 * PIXELS_PER_UNIT, WHITE);
        if app.inst.running() {
            let to_screen =
                |x: f64, y: f64| (cx + x as f32 * PIXELS_PER_UNIT, cy - y as f32 * PIXELS_PER_UNIT);
            // event flashes: expanding rings read straight from the event
            // log (stateless — rewind and they replay with the timeline)
            let now = app.inst.tick().unwrap_or(0);
            for ev in app.inst.recent_events(24) {
                let k = now.saturating_sub(ev.tick) as f32 / 24.0;
                let (col, r0) = match &*ev.name {
                    "graze" => (Color::new(0.6, 0.9, 1.0, 0.7 * (1.0 - k)), 6.0),
                    "player-hit" => (Color::new(1.0, 0.25, 0.3, 0.9 * (1.0 - k)), 12.0),
                    "enemy-hit" => (Color::new(1.0, 0.8, 0.3, 0.5 * (1.0 - k)), 8.0),
                    "died" => (Color::new(1.0, 0.6, 0.2, 0.8 * (1.0 - k)), 12.0),
                    _ => continue,
                };
                if let Some((ex, ey)) = ev.pos {
                    let (sx, sy) = to_screen(ex, ey);
                    draw_circle_lines(sx, sy, r0 + k * 26.0, 2.0, col);
                }
            }
            // post-hit iframes: flash the player marker
            if app.inst.iframes_active() && (now / 6) % 2 == 0 {
                draw_circle_lines(pmx, pmy, 14.0, 2.0, Color::new(1.0, 0.3, 0.3, 0.8));
            }

            // scrub indicators: where the position channels ARE at this tick
            // (while paused they diverge from the live mouse)
            if app.inst.paused() {
                for (name, col) in
                    [("player", Color::new(1.0, 1.0, 1.0, 0.9)), ("nearest-enemy", ORANGE)]
                {
                    if let Some(Val::Pose(p)) = app.inst.channel(name) {
                        let (sx, sy) = to_screen(p.x, p.y);
                        draw_line(sx - 10.0, sy, sx + 10.0, sy, 1.5, col);
                        draw_line(sx, sy - 10.0, sx, sy + 10.0, 1.5, col);
                        draw_circle_lines(sx, sy, 6.0, 1.5, col);
                        draw_text(&format!("${}", name), sx + 12.0, sy - 8.0, 16.0, col);
                    }
                }
            }
            draw_text(
                &format!(
                    "{}  tick {}  entities {}  graze {}  hits {}  lives {}  {}",
                    app.inst.status(),
                    now,
                    app.inst.entity_count(),
                    app.inst.graze(),
                    app.inst.player_hits(),
                    match app.inst.channel("lives") {
                        Some(Val::Num(n)) => format!("{}", n),
                        _ => "-".into(),
                    },
                    if app.inst.paused() { "[paused]" } else { "" }
                ),
                12.0,
                24.0,
                22.0,
                GRAY,
            );
        } else {
            draw_text(app.inst.status(), 12.0, 24.0, 22.0, RED);
        }
        // pattern menu (above the timeline strip): every entry is a
        // click target — hotkeys 1-9 only reach the first nine
        let current = app.inst.current_pattern().unwrap_or_default();
        let n_patterns = app.inst.patterns().len();
        let names: Vec<String> = app.inst.patterns().to_vec();
        let mut clicked: Option<usize> = None;
        for (i, name) in names.iter().enumerate() {
            let sel = *name == current;
            let y = screen_height() - TIMELINE_H - 14.0 * (n_patterns - i) as f32;
            let label = if i < 9 {
                format!("{} {}", i + 1, name)
            } else {
                format!("  {}", name)
            };
            let w = measure_text(&label, None, 18, 1.0).width;
            let hover = mx >= 12.0 && mx <= 12.0 + w && my >= y - 14.0 && my <= y + 4.0;
            if hover && is_mouse_button_pressed(MouseButton::Left) {
                clicked = Some(i);
            }
            draw_text(
                &label,
                12.0,
                y,
                18.0,
                if sel {
                    WHITE
                } else if hover {
                    YELLOW
                } else {
                    GRAY
                },
            );
        }
        if let Some(i) = clicked {
            app.inst.select(i);
        }
        draw_text(
            &format!("rank {:.1} (T/Y/U/I)  [B]indings", app.binds.get_const("rank").unwrap_or(1.0)),
            screen_width() - 230.0,
            24.0,
            18.0,
            GRAY,
        );
        timeline_ui(&mut app, mx, my);
        app.binds.ui();
        next_frame().await;
    }
}
