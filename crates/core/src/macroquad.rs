//! Macroquad compatibility adapter for host-neutral Touhou pack frames.
//!
//! Sprite instances are expanded to quads and `u32` ribbon indices are
//! remapped to Macroquad's `u16` mesh format before CPU submission.

use crate::touhou::{AddressMode, BlendMode, DrawSource, MaterialDesc, MaterialId, MeshFrame, SourceLayout, TextureFilter, TextureSource, TouhouProfile};
use macroquad::miniquad::{Backend, BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
use macroquad::prelude::*;
use std::collections::HashMap;

const PIXELS_PER_UNIT: f32 = 55.0;

pub struct RenderResources { textures: Vec<Texture2D>, materials: Vec<MaterialDesc>, pipelines: Vec<Material> }
impl RenderResources {
    pub fn resolve(profile: &TouhouProfile) -> Result<Self, String> {
        let mut textures=Vec::with_capacity(profile.textures().len());
        for texture in profile.textures() { textures.push(match &texture.source {
            TextureSource::BuiltinRgba8{width,height,bytes} => Texture2D::from_rgba8(u16::try_from(*width).map_err(|_|format!("texture '{}' is too wide",texture.key))?,u16::try_from(*height).map_err(|_|format!("texture '{}' is too high",texture.key))?,bytes),
            TextureSource::External{key} => return Err(format!("external texture '{}' is not registered in the debug player",key)),
        }); }
        let mut pipelines=Vec::with_capacity(profile.materials().len()); let mut filters=vec![None;textures.len()];
        for material in profile.materials() {
            if material.texture.0 as usize>=textures.len(){return Err(format!("material '{}' has no resolved texture",material.key));}
            if !matches!(material.pipeline.as_ref(),"touhou.sprite.v1"|"touhou.ribbon.v1"){return Err(format!("debug player has no registered pipeline '{}'",material.pipeline));}
            if material.sampler.address_u!=AddressMode::Clamp||material.sampler.address_v!=AddressMode::Clamp{return Err(format!("debug player material '{}' requires unsupported non-clamp addressing",material.key));}
            if material.sampler.min_filter!=material.sampler.mag_filter{return Err(format!("debug player material '{}' requires distinct min/mag filters",material.key));}
            let filter=match material.sampler.min_filter{TextureFilter::Nearest=>FilterMode::Nearest,TextureFilter::Linear=>FilterMode::Linear};
            let slot=&mut filters[material.texture.0 as usize]; if slot.is_some_and(|v|v!=filter){return Err(format!("texture {} is referenced with conflicting filters",material.texture.0));} *slot=Some(filter);
            pipelines.push(resolve_pipeline(material)?);
        }
        for(texture,filter)in textures.iter().zip(filters){texture.set_filter(filter.unwrap_or(FilterMode::Linear));}
        Ok(Self{textures,materials:profile.materials().to_vec(),pipelines})
    }
    fn material(&self,id:MaterialId)->(&MaterialDesc,&Texture2D,&Material){let i=id.0 as usize;let m=&self.materials[i];(m,&self.textures[m.texture.0 as usize],&self.pipelines[i])}
}

pub struct PreparedDraw { material: MaterialId, meshes: Vec<Mesh> }
pub struct PreparedFrame { draws: Vec<PreparedDraw> }
impl PreparedFrame {
    pub fn draw_count(&self)->usize{self.draws.len()}
    pub fn mesh_count(&self)->usize{self.draws.iter().map(|d|d.meshes.len()).sum()}
    pub fn vertex_count(&self)->usize{self.draws.iter().flat_map(|d|&d.meshes).map(|m|m.vertices.len()).sum()}
    pub fn index_count(&self)->usize{self.draws.iter().flat_map(|d|&d.meshes).map(|m|m.indices.len()).sum()}
}

pub fn prepare_frame(frame:&MeshFrame,resources:&RenderResources,cx:f32,cy:f32)->PreparedFrame{
    let mut draws=Vec::with_capacity(frame.draws.len());
    for command in &frame.draws {
        let(material,texture,_)=resources.material(command.material); debug_assert_eq!(material.layout,command.source.layout());
        let meshes=match command.source{
            DrawSource::BasicSprites{start,count}=>{let fixed=material.fixed_color.expect("validated basic material").0;sprite_meshes(frame.basic_sprites[start as usize..(start+count)as usize].iter().map(|v|{let alpha=((fixed[3]as u16*v.alpha as u16+127)/255)as u8;(v,[fixed[0],fixed[1],fixed[2],alpha],[0.0;4])}),texture,cx,cy)},
            DrawSource::TintedSprites{start,count}=>sprite_meshes(frame.tinted_sprites[start as usize..(start+count)as usize].iter().map(|v|(&v.base,v.tint,[0.0;4])),texture,cx,cy),
            DrawSource::RecolorSprites{start,count}=>sprite_meshes(frame.recolor_sprites[start as usize..(start+count)as usize].iter().map(|v|(&v.base,v.color_lo,v.color_hi.map(|c|c as f32/255.0))),texture,cx,cy),
            DrawSource::Indexed{index_start,index_count,..}=>indexed_meshes(frame,index_start,index_count,texture,cx,cy),
        }; draws.push(PreparedDraw{material:command.material,meshes});
    } PreparedFrame{draws}
}

pub fn submit_frame(frame:&PreparedFrame,resources:&RenderResources){
    for draw in &frame.draws { let(_,_,pipeline)=resources.material(draw.material); gl_use_material(pipeline); for mesh in &draw.meshes{draw_mesh(mesh);} gl_use_default_material(); }
}
pub fn draw_frame(frame:&MeshFrame,resources:&RenderResources,cx:f32,cy:f32){let prepared=prepare_frame(frame,resources,cx,cy);submit_frame(&prepared,resources);}

fn sprite_meshes<'a>(instances:impl Iterator<Item=(&'a crate::touhou::BasicSpriteInstance,[u8;4],[f32;4])>,texture:&Texture2D,cx:f32,cy:f32)->Vec<Mesh>{
    const QUADS:usize=u16::MAX as usize/4; let mut out=Vec::new();let mut vertices=Vec::with_capacity(QUADS*4);let mut indices=Vec::with_capacity(QUADS*6);
    for(instance,color,normal)in instances{
        if vertices.len()+4>u16::MAX as usize{out.push(Mesh{vertices,indices,texture:Some(texture.clone())});vertices=Vec::with_capacity(QUADS*4);indices=Vec::with_capacity(QUADS*6);}
        let base=vertices.len()as u16;let(s,c)=instance.rotation.to_radians().sin_cos();let[u0,v0,u1,v1]=instance.uv_rect;
        for([lx,ly],uv)in[([-instance.half_size[0],-instance.half_size[1]],[u0,v0]),([instance.half_size[0],-instance.half_size[1]],[u1,v0]),([instance.half_size[0],instance.half_size[1]],[u1,v1]),([-instance.half_size[0],instance.half_size[1]],[u0,v1])]{let wx=instance.center[0]+c*lx-s*ly;let wy=instance.center[1]+s*lx+c*ly;vertices.push(Vertex{position:vec3(cx+wx*PIXELS_PER_UNIT,cy-wy*PIXELS_PER_UNIT,0.0),uv:vec2(uv[0],uv[1]),color,normal:vec4(normal[0],normal[1],normal[2],normal[3])});}
        indices.extend_from_slice(&[base,base+1,base+2,base,base+2,base+3]);
    } if!indices.is_empty(){out.push(Mesh{vertices,indices,texture:Some(texture.clone())});} out
}
fn indexed_meshes(frame:&MeshFrame,index_start:u32,index_count:u32,texture:&Texture2D,cx:f32,cy:f32)->Vec<Mesh>{
    let mut out=Vec::new();let mut vertices=Vec::new();let mut indices=Vec::new();let mut remap=HashMap::<u32,u16>::new();
    for tri in frame.indices[index_start as usize..(index_start+index_count)as usize].chunks_exact(3){let fresh=tri.iter().filter(|i|!remap.contains_key(i)).count();if!indices.is_empty()&&vertices.len()+fresh>u16::MAX as usize{out.push(Mesh{vertices,indices,texture:Some(texture.clone())});vertices=Vec::new();indices=Vec::new();remap.clear();}for source in tri{let local=*remap.entry(*source).or_insert_with(||{let v=frame.vertices[*source as usize];let local=vertices.len()as u16;vertices.push(Vertex{position:vec3(cx+v.pos[0]*PIXELS_PER_UNIT,cy-v.pos[1]*PIXELS_PER_UNIT,0.0),uv:vec2(v.uv[0],v.uv[1]),color:v.color,normal:vec4(0.0,0.0,0.0,0.0)});local});indices.push(local);}}
    if!indices.is_empty(){out.push(Mesh{vertices,indices,texture:Some(texture.clone())});}out
}

fn resolve_pipeline(desc:&MaterialDesc)->Result<Material,String>{
    let blend=match desc.blend{BlendMode::Opaque=>None,BlendMode::Alpha=>Some(BlendState::new(Equation::Add,BlendFactor::Value(BlendValue::SourceAlpha),BlendFactor::OneMinusValue(BlendValue::SourceAlpha))),BlendMode::Additive=>Some(BlendState::new(Equation::Add,BlendFactor::Value(BlendValue::SourceAlpha),BlendFactor::One)),BlendMode::SoftAdditive=>Some(BlendState::new(Equation::Add,BlendFactor::OneMinusValue(BlendValue::DestinationColor),BlendFactor::One))};
    let params=MaterialParams{pipeline_params:PipelineParams{color_blend:blend,alpha_blend:blend,..Default::default()},..Default::default()};let recolor=desc.layout==SourceLayout::RecolorSprite;
    let backend=unsafe{get_internal_gl().quad_context.info().backend};let shader=match backend{Backend::OpenGl=>ShaderSource::Glsl{vertex:VERTEX,fragment:if recolor{RECOLOR}else{STANDARD}},Backend::Metal=>ShaderSource::Msl{program:if recolor{RECOLOR_MSL}else{STANDARD_MSL}}};load_material(shader,params).map_err(|e|format!("material '{}': {e}",desc.key))
}
const VERTEX:&str=r#"#version 100
attribute vec3 position; attribute vec2 texcoord; attribute vec4 color0; attribute vec4 normal; varying lowp vec2 uv; varying lowp vec4 color; varying lowp vec4 recolor_high; uniform mat4 Model; uniform mat4 Projection;
void main(){gl_Position=Projection*Model*vec4(position,1.0);uv=texcoord;color=color0/255.0;recolor_high=normal;}"#;
const STANDARD:&str=r#"#version 100
varying lowp vec2 uv; varying lowp vec4 color; uniform sampler2D Texture; void main(){gl_FragColor=color*texture2D(Texture,uv);}"#;
const RECOLOR:&str=r#"#version 100
varying lowp vec2 uv; varying lowp vec4 color; varying lowp vec4 recolor_high; uniform sampler2D Texture; void main(){lowp vec4 sample=texture2D(Texture,uv);gl_FragColor=mix(color,recolor_high,sample.r)*sample.a;}"#;
const STANDARD_MSL:&str=r#"#include <metal_stdlib>
using namespace metal; struct Uniforms{float4x4 Model;float4x4 Projection;};struct Vertex{float3 position[[attribute(0)]];float2 uv[[attribute(1)]];float4 color[[attribute(2)]];float4 normal[[attribute(3)]];};struct Raster{float4 position[[position]];float2 uv[[user(locn0)]];float4 color[[user(locn1)]];float4 high[[user(locn2)]];};vertex Raster vertexShader(Vertex v[[stage_in]],constant Uniforms&u[[buffer(0)]]){Raster o;o.position=u.Projection*u.Model*float4(v.position,1);o.uv=v.uv;o.color=v.color/255.0;o.high=v.normal;return o;}fragment float4 fragmentShader(Raster in[[stage_in]],texture2d<float>tex[[texture(0)]],sampler smp[[sampler(0)]]){return in.color*tex.sample(smp,in.uv);}"#;
const RECOLOR_MSL:&str=r#"#include <metal_stdlib>
using namespace metal; struct Uniforms{float4x4 Model;float4x4 Projection;};struct Vertex{float3 position[[attribute(0)]];float2 uv[[attribute(1)]];float4 color[[attribute(2)]];float4 normal[[attribute(3)]];};struct Raster{float4 position[[position]];float2 uv[[user(locn0)]];float4 color[[user(locn1)]];float4 high[[user(locn2)]];};vertex Raster vertexShader(Vertex v[[stage_in]],constant Uniforms&u[[buffer(0)]]){Raster o;o.position=u.Projection*u.Model*float4(v.position,1);o.uv=v.uv;o.color=v.color/255.0;o.high=v.normal;return o;}fragment float4 fragmentShader(Raster in[[stage_in]],texture2d<float>tex[[texture(0)]],sampler smp[[sampler(0)]]){float4 sample=tex.sample(smp,in.uv);return mix(in.color,in.high,sample.r)*sample.a;}"#;
