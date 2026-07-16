use super::*;
use maku::host::Instance;
use maku::render::{Column, NumColumn, RenderBatch, RenderData, RenderFieldKind, RenderItem, RenderRow, RenderSchema};
use maku::host::Inputs;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;

struct CountingAllocator;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTING.with(|on| if on.get() { ALLOCS.with(|n| n.set(n.get() + 1)); });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout) } }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNTING.with(|on| if on.get() { ALLOCS.with(|n| n.set(n.get() + 1)); });
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn sprite_schema() -> Rc<RenderSchema> {
    Rc::new(RenderSchema { cols: vec![
        (Rc::from("family"), RenderFieldKind::Sym),
        (Rc::from("color"), RenderFieldKind::Sym),
        (Rc::from("variant"), RenderFieldKind::Sym),
    ]})
}

fn beam_schema() -> Rc<RenderSchema> {
    Rc::new(RenderSchema { cols: vec![
        (Rc::from("width"), RenderFieldKind::Num),
        (Rc::from("family"), RenderFieldKind::Sym),
        (Rc::from("color"), RenderFieldKind::Sym),
        (Rc::from("variant"), RenderFieldKind::Sym),
    ]})
}

fn bind_handlers(mesh: &mut TouhouMesh) {
    mesh.bind_schema("sprite", sprite_schema()).unwrap();
    mesh.bind_schema("beam", beam_schema()).unwrap();
}

fn bound_mesh() -> TouhouMesh {
    let mut mesh = TouhouMesh::default();
    bind_handlers(&mut mesh);
    mesh
}

fn bound_profile_mesh(profile: Rc<TouhouProfile>) -> TouhouMesh {
    let mut mesh = TouhouMesh::new(profile);
    bind_handlers(&mut mesh);
    mesh
}

fn sprite_row(family: &str, color: &str, variant: &str, theta: f64, scale: f64) -> Rc<RenderRow> {
    Rc::new(RenderRow {
        kind: Rc::from("sprite"),
        data: RenderData::Point { x: 1.0, y: 2.0, theta, scale, alpha: 0.5, hue: 0.0 },
        nums: vec![],
        syms: vec![
            (Rc::from("family"), Rc::from(family)),
            (Rc::from("color"), Rc::from(color)),
            (Rc::from("variant"), Rc::from(variant)),
        ],
    })
}

fn sprite_batch() -> Rc<RenderBatch> {
    Rc::new(RenderBatch {
        kind: Rc::from("sprite"), schema: sprite_schema(), len: 1,
        x: NumColumn::Const(1.0), y: NumColumn::Const(2.0), theta: NumColumn::Const(0.0),
        scale: NumColumn::Const(1.0), alpha: NumColumn::Const(0.5), hue: NumColumn::Const(0.0),
        cols: vec![Column::SymConst(Rc::from("star")), Column::SymConst(Rc::from("red")), Column::SymConst(Rc::from(""))],
    })
}

fn beam_row(width: f64, active: bool) -> Rc<RenderRow> {
    Rc::new(RenderRow {
        kind: Rc::from("beam"),
        data: RenderData::Polyline { points: vec![(0.0, 0.0), (1.0, 0.0)], active },
        nums: vec![(Rc::from("width"), width), (Rc::from("alpha"), 1.0), (Rc::from("hue"), 0.0)],
        syms: vec![(Rc::from("family"), Rc::from("laser")), (Rc::from("color"), Rc::from("blue")), (Rc::from("variant"), Rc::from(""))],
    })
}

#[test]
fn stock_profile_owns_resources_materials_and_layers() {
    let profile = TouhouProfile::stock();
    assert_eq!(profile.pixels_per_unit(), 55.0);
    assert!(profile.palettes().iter().any(|p| p.key.as_ref() == "red" && p.pure == Rgba8::rgb(0xff, 0x4d, 0x5e)));
    assert_eq!(profile.sprite(profile.sprite_id("star", "").unwrap()).radius_world, 5.0 / 55.0);
    assert_eq!(profile.sprite(profile.sprite_id("lstar", "").unwrap()).layers.len(), 2);
    assert_eq!(profile.beam(profile.beam_id("laser", "").unwrap()).layers[0].active.width_px, 6.0);
    match &profile.textures()[0].source {
        TextureSource::BuiltinRgba8 { width, height, bytes } => {
            assert_eq!((*width, *height, bytes.len()), (64, 64, 64 * 64 * 4));
            assert!(bytes.chunks_exact(4).any(|p| p[3] != 0));
        }
        _ => panic!("stock atlas must be builtin"),
    }
}

#[test]
fn custom_external_resource_material_and_recolor_layout_are_registered() {
    let mut desc = TouhouProfile::stock_desc();
    desc.textures.push(TextureResource {
        key: Rc::from("host.sheet"),
        source: TextureSource::External { key: Rc::from("asset://bullets.png") },
    });
    let material_id = MaterialId(desc.materials.len() as u32);
    desc.materials.push(MaterialDesc {
        key: Rc::from("host.recolor"), primitive: PrimitiveClass::Sprites,
        layout: SourceLayout::RecolorSprite, texture: TextureId(1),
        pipeline: Rc::from("host.recolor.v1"), blend: BlendMode::Alpha,
        sampler: SamplerDesc::default(), fixed_color: None,
    });
    desc.sprites.push(SpriteStyle {
        family: Rc::from("host"), variant: Rc::from("gradient"), radius_world: 0.2,
        orientation: OrientationPolicy::Directional,
        layers: vec![SpriteLayer {
            material: material_id,
            region: TextureRegion { texture: TextureId(1), uv: [0.0, 0.0, 1.0, 1.0] },
            size_mul: [1.0, 2.0], angle_offset: 10.0, alpha_mul: 1.0,
            color: LayerColor::Recolor { low: PaletteShade::Dark, high: PaletteShade::Highlight },
        }],
    });
    let profile = Rc::new(TouhouProfile::new(desc).unwrap());
    assert!(matches!(profile.textures()[1].source, TextureSource::External { .. }));
    let mut mesh = bound_profile_mesh(profile);
    let out = mesh.build(&[RenderItem::Row(sprite_row("host", "red", "gradient", 20.0, 1.0))]).unwrap();
    assert_eq!(out.recolor_sprites.len(), 1);
    assert_eq!(out.recolor_sprites[0].base.rotation, 30.0);
    assert_eq!(out.draws[0].material, material_id);
}

#[test]
fn profile_rejects_duplicate_dangling_layout_region_and_effect_capability() {
    let mut desc = TouhouProfile::stock_desc();
    desc.palettes.push(desc.palettes[0].clone());
    desc.materials[0].texture = TextureId(99);
    desc.sprites[0].layers[0].region.uv = [-1.0, 0.0, 1.0, 1.0];
    desc.fallback_color = Rc::from("missing");
    desc.effects.push(LocalEffect::SpriteLayers {
        target: StyleKey::new("default", ""),
        required: PrimitiveCapabilities { channels: 1 << 30, ..Default::default() },
        insertion: LayerInsertion::After,
        layers: vec![],
    });
    let error = TouhouProfile::new(desc).unwrap_err().to_string();
    assert!(error.contains("duplicate palette"));
    assert!(error.contains("missing texture"));
    assert!(error.contains("invalid texture region"));
    assert!(error.contains("unsupported capabilities"));
    assert!(error.contains("fallback_color"));
}

#[test]
fn profile_validation_covers_all_cold_contract_boundaries() {
    fn rejected(mutator: impl FnOnce(&mut ProfileDesc), needle: &str) {
        let mut desc = TouhouProfile::stock_desc();
        mutator(&mut desc);
        let message = TouhouProfile::new(desc).unwrap_err().to_string();
        assert!(message.contains(needle), "expected '{needle}' in {message}");
    }
    rejected(|d| d.pixels_per_unit = 0.0, "pixels_per_unit");
    rejected(|d| d.textures.push(d.textures[0].clone()), "duplicate texture");
    rejected(|d| d.materials.push(d.materials[0].clone()), "duplicate material");
    rejected(|d| d.sprites.push(d.sprites[0].clone()), "duplicate sprite");
    rejected(|d| d.beams.push(d.beams[0].clone()), "duplicate beam");
    rejected(|d| if let TextureSource::BuiltinRgba8 { bytes, .. } = &mut d.textures[0].source { *bytes = Vec::new().into_boxed_slice(); }, "byte length");
    rejected(|d| d.materials[0].layout = SourceLayout::IndexedStrip, "incompatible primitive/layout");
    rejected(|d| d.materials[1].fixed_color = None, "fixed_color");
    rejected(|d| d.sprites[0].layers[1].color = LayerColor::Fixed(Rgba8::rgb(10, 20, 30)), "fixed color differs");
    rejected(|d| d.sprites[0].radius_world = f32::NAN, "radius");
    rejected(|d| d.sprites[0].layers[0].material = MaterialId(999), "missing material");
    rejected(|d| d.sprites[0].layers[0].size_mul[0] = 0.0, "dimensions/alpha/angle");
    rejected(|d| d.beams[0].layers[0].active.width_px = 0.0, "invalid width/alpha");
    rejected(|d| d.beams[0].layers[0].join = JoinPolicy::Bevel, "unsupported by ribbon v1");
    rejected(|d| d.fallback_sprite = StyleKey::new("missing", ""), "fallback_sprite");
    rejected(|d| d.fallback_beam = StyleKey::new("missing", ""), "fallback_beam");
}

#[test]
fn schema_binding_validates_names_and_kinds_and_is_cached() {
    let mut mesh = bound_mesh();
    let schema = sprite_schema();
    mesh.bind_schema("sprite", schema.clone()).unwrap();
    mesh.bind_schema("sprite", schema).unwrap();
    mesh.bind_schema("beam", Rc::new(RenderSchema { cols: vec![
        (Rc::from("width"), RenderFieldKind::Num),
        (Rc::from("hue"), RenderFieldKind::Num),
        (Rc::from("family"), RenderFieldKind::Sym),
        (Rc::from("color"), RenderFieldKind::Sym),
        (Rc::from("variant"), RenderFieldKind::Sym),
    ]})).unwrap();
    let bad = Rc::new(RenderSchema { cols: vec![
        (Rc::from("family"), RenderFieldKind::Num),
        (Rc::from("color"), RenderFieldKind::Sym),
        (Rc::from("variant"), RenderFieldKind::Sym),
    ]});
    assert!(mesh.bind_schema("sprite", bad).unwrap_err().to_string().contains("family"));
    let bad_beam = Rc::new(RenderSchema { cols: vec![
        (Rc::from("width"), RenderFieldKind::Sym),
        (Rc::from("hue"), RenderFieldKind::Num),
        (Rc::from("family"), RenderFieldKind::Sym),
        (Rc::from("color"), RenderFieldKind::Sym),
        (Rc::from("variant"), RenderFieldKind::Sym),
    ]});
    assert!(mesh.bind_schema("beam", bad_beam).unwrap_err().to_string().contains("width"));
}

#[test]
fn foreign_undeclared_kinds_are_not_reinterpreted_by_the_pack() {
    let row = Rc::new(RenderRow::of_kind(Rc::from("foreign"), RenderData::Point {
        x: 0.0, y: 0.0, theta: 0.0, scale: 1.0, alpha: 1.0, hue: 0.0,
    }));
    let mut mesh = bound_mesh();
    assert_eq!(mesh.build(&[RenderItem::Row(row)]).unwrap(), &MeshFrame::default());
}

#[test]
fn row_and_batch_are_byte_equivalent_without_expansion() {
    let row = sprite_row("star", "red", "", 0.0, 1.0);
    let batch = sprite_batch();
    let mut mesh = bound_mesh();
    let row_out = mesh.build(&[RenderItem::Row(row)]).unwrap();
    let expected = (
        row_out.basic_sprites.clone(), row_out.tinted_sprites.clone(), row_out.recolor_sprites.clone(),
        row_out.vertices.clone(), row_out.indices.clone(), row_out.draws.clone(),
    );
    let batch_out = mesh.build(&[RenderItem::Batch(batch)]).unwrap();
    assert_eq!(expected, (
        batch_out.basic_sprites.clone(), batch_out.tinted_sprites.clone(), batch_out.recolor_sprites.clone(),
        batch_out.vertices.clone(), batch_out.indices.clone(), batch_out.draws.clone(),
    ));
}

#[test]
fn stock_fill_outline_color_alpha_and_direction_are_compiled_data() {
    let mut mesh = bound_mesh();
    let radial = mesh.build(&[RenderItem::Row(sprite_row("star", "red", "", 73.0, 2.0))]).unwrap();
    assert_eq!(radial.tinted_sprites[0].tint, [0xff, 0x4d, 0x5e, 128]);
    assert_eq!(radial.tinted_sprites[0].base.half_size, [10.0 / 55.0; 2]);
    assert_eq!(radial.tinted_sprites[0].base.rotation, 0.0);
    assert_eq!(radial.basic_sprites[0].alpha, 45);
    assert_eq!(radial.draws.iter().map(|d| d.material).collect::<Vec<_>>(), vec![MaterialId(0), MaterialId(1)]);

    let directional = mesh.build(&[RenderItem::Row(sprite_row("arrow", "red", "", 73.0, 1.0))]).unwrap();
    assert_eq!(directional.tinted_sprites[0].base.rotation, 73.0);
}

#[test]
fn profile_palette_hue_and_alpha_are_applied_together() {
    let row = Rc::new(RenderRow {
        kind: Rc::from("sprite"),
        data: RenderData::Point { x: 0.0, y: 0.0, theta: 0.0, scale: 1.0, alpha: 0.25, hue: 60.0 },
        nums: vec![],
        syms: vec![
            (Rc::from("family"), Rc::from("star")),
            (Rc::from("color"), Rc::from("red")),
            (Rc::from("variant"), Rc::from("")),
        ],
    });
    let mut mesh = bound_mesh();
    let out = mesh.build(&[RenderItem::Row(row)]).unwrap();
    assert_ne!(&out.tinted_sprites[0].tint[..3], &[0xff, 0x4d, 0x5e]);
    assert_eq!(out.tinted_sprites[0].tint[3], 64);
    assert_eq!(out.basic_sprites[0].alpha, 22);
}

#[test]
fn strict_unknown_errors_and_fallback_diagnostics_are_explicit() {
    let mut fallback = bound_mesh();
    fallback.build(&[RenderItem::Row(sprite_row("mystery", "chartreuse", "x", 0.0, 1.0))]).unwrap();
    assert_eq!(fallback.diagnostics().len(), 2);
    fallback.build(&[RenderItem::Row(sprite_row("mystery", "chartreuse", "x", 0.0, 1.0))]).unwrap();
    assert_eq!(fallback.diagnostics().len(), 2);

    let mut desc = TouhouProfile::stock_desc();
    desc.unknown = UnknownStylePolicy::Error;
    let mut strict = bound_profile_mesh(Rc::new(TouhouProfile::new(desc).unwrap()));
    assert!(matches!(strict.build(&[RenderItem::Row(sprite_row("mystery", "red", "", 0.0, 1.0))]), Err(RenderError::UnknownStyle { .. })));
}

#[test]
fn replacing_profile_clears_bindings_diagnostics_and_output() {
    let mut mesh = bound_mesh();
    mesh.bind_schema("sprite", sprite_schema()).unwrap();
    mesh.build(&[RenderItem::Row(sprite_row("missing", "red", "", 0.0, 1.0))]).unwrap();
    assert!(!mesh.diagnostics().is_empty());
    mesh.replace_profile(Rc::new(TouhouProfile::stock()));
    assert!(mesh.diagnostics().is_empty());
    assert_eq!(mesh.build(&[]).unwrap(), &MeshFrame::default());
}

#[test]
fn finite_f64_values_that_overflow_f32_are_rejected() {
    let mut mesh = bound_mesh();
    let row = Rc::new(RenderRow {
        kind: Rc::from("sprite"),
        data: RenderData::Point { x: f64::MAX, y: 0.0, theta: 0.0, scale: 1.0, alpha: 1.0, hue: 0.0 },
        nums: vec![],
        syms: vec![(Rc::from("family"), Rc::from("star")), (Rc::from("color"), Rc::from("red")), (Rc::from("variant"), Rc::from(""))],
    });
    assert!(matches!(mesh.build(&[RenderItem::Row(row)]), Err(RenderError::InvalidRow(_))));
}

#[test]
fn beam_width_warning_and_order_are_profile_driven() {
    let mut mesh = bound_mesh();
    let active = mesh.build(&[RenderItem::Row(beam_row(2.0, true))]).unwrap().vertices.clone();
    let warning = mesh.build(&[RenderItem::Row(beam_row(2.0, false))]).unwrap().vertices.clone();
    assert!((active[0].pos[1].abs() - 6.0 / 55.0).abs() < 1e-6);
    assert!((warning[0].pos[1].abs() - 1.5 / 55.0).abs() < 1e-6);
    assert_eq!(warning[0].color[3], 115);
}

#[test]
fn compatible_sprite_and_ribbon_effects_compile_to_layers() {
    let mut desc = TouhouProfile::stock_desc();
    let halo = SpriteLayer {
        material: MaterialId(4), region: TextureRegion { texture: TextureId(0), uv: [0.0, 0.0, 0.5, 0.5] },
        size_mul: [1.5, 1.5], angle_offset: 0.0, alpha_mul: 0.3,
        color: LayerColor::Tint(PaletteShade::Light),
    };
    let beam_halo = RibbonLayer {
        material: MaterialId(5), region: TextureRegion { texture: TextureId(0), uv: [0.24, 0.24, 0.26, 0.26] },
        width_mul: 1.8,
        active: RibbonAppearance { width_px: 6.0, alpha_mul: 0.3 },
        warning: RibbonAppearance { width_px: 1.5, alpha_mul: 0.2 },
        color: LayerColor::Tint(PaletteShade::Light), join: JoinPolicy::Segment, cap: CapPolicy::Butt,
    };
    let req = PrimitiveCapabilities { operations: PrimitiveCapabilities::OP_DUPLICATE | PrimitiveCapabilities::OP_SCALE, ..Default::default() };
    desc.effects.push(LocalEffect::SpriteLayers {
        target: StyleKey::new("star", ""), required: req,
        insertion: LayerInsertion::After, layers: vec![halo],
    });
    desc.effects.push(LocalEffect::RibbonLayers {
        target: StyleKey::new("laser", ""), required: req,
        insertion: LayerInsertion::Before, layers: vec![beam_halo],
    });
    let mut mesh = bound_profile_mesh(Rc::new(TouhouProfile::new(desc).unwrap()));
    let sprite_materials = mesh.build(&[RenderItem::Row(sprite_row("star", "red", "", 0.0, 1.0))]).unwrap()
        .draws.iter().map(|d| d.material).collect::<Vec<_>>();
    assert_eq!(sprite_materials, vec![MaterialId(0), MaterialId(1), MaterialId(4)]);
    let beam_materials = mesh.build(&[RenderItem::Row(beam_row(1.0, true))]).unwrap()
        .draws.iter().map(|d| d.material).collect::<Vec<_>>();
    assert_eq!(beam_materials, vec![MaterialId(5), MaterialId(3)]);
}

#[test]
fn mixed_handlers_preserve_order_and_only_adjacent_sources_coalesce() {
    let mut mesh = bound_mesh();
    let items = [
        RenderItem::Row(sprite_row("star", "red", "", 0.0, 1.0)),
        RenderItem::Row(beam_row(1.0, true)),
        RenderItem::Row(sprite_row("star", "red", "", 0.0, 1.0)),
    ];
    let out = mesh.build(&items).unwrap();
    assert_eq!(out.draws.iter().map(|d| d.material).collect::<Vec<_>>(),
        vec![MaterialId(0), MaterialId(1), MaterialId(3), MaterialId(0), MaterialId(1)]);

    let mut desc = TouhouProfile::stock_desc();
    desc.sprites.iter_mut().for_each(|s| s.layers.truncate(1));
    let mut one_layer = bound_profile_mesh(Rc::new(TouhouProfile::new(desc).unwrap()));
    let out = one_layer.build(&[
        RenderItem::Row(sprite_row("star", "red", "", 0.0, 1.0)),
        RenderItem::Row(sprite_row("star", "blue", "", 0.0, 1.0)),
    ]).unwrap();
    assert_eq!(out.draws, vec![DrawCommand { material: MaterialId(0), source: DrawSource::TintedSprites { start: 0, count: 2 } }]);
}

#[test]
fn wasm_visible_v1_struct_strides_are_stable() {
    assert_eq!(std::mem::size_of::<BasicSpriteInstance>(), 40);
    assert_eq!(std::mem::size_of::<TintedSpriteInstance>(), 44);
    assert_eq!(std::mem::size_of::<RecolorSpriteInstance>(), 48);
    assert_eq!(std::mem::size_of::<StripVertex>(), 20);
}

#[test]
fn source_layouts_are_sparse_and_planning_is_visible() {
    let mut mesh = bound_mesh();
    let batch = sprite_batch();
    let plan = mesh.plan_batch(&batch).unwrap();
    assert_eq!((plan.rows, plan.total_instances, plan.fixed_layers_per_row), (1, 2, Some(2)));
    assert_eq!(plan.instances_by_layout, [1, 1, 0]);
    assert_ne!(plan.source_layouts & PrimitiveCapabilities::LAYOUT_TINT, 0);
    let out = mesh.build(&[RenderItem::Batch(batch)]).unwrap();
    assert_eq!((out.basic_sprites.len(), out.tinted_sprites.len(), out.recolor_sprites.len()), (1, 1, 0));
    assert_eq!(TouhouMesh::ribbon_segment_count(&[(0.0, 0.0), (0.0, 0.0), (1.0, 0.0)]), 1);
}

#[test]
fn stock_profile_renders_a_real_declared_touhou_card() {
    let card = format!("{}/../../cards/tutorials/t03.maku", env!("CARGO_MANIFEST_DIR"));
    if !std::path::Path::new(&card).exists() {
        return; // repository-level integration fixture is not in the crate archive
    }
    let mut inst = Instance::new(None);
    inst.set_render_kinds(TouhouMesh::RENDER_KINDS.iter().copied());
    inst.boot(card, Some("ex3-fruit-colors".into()));
    for _ in 0..300 { inst.advance(Inputs::default()); }
    assert!(inst.running(), "{}", inst.status());
    let mut mesh = bound_mesh();
    for kind in ["sprite", "beam"] {
        if let Some(schema) = inst.declared_render_schema(kind) {
            mesh.bind_schema(kind, schema.clone()).unwrap_or_else(|e| panic!("{kind} {:?}: {e}", schema.cols));
        }
    }
    let frame = inst.render_frame();
    let out = mesh.build(&frame).unwrap();
    assert!(!out.draws.is_empty());
    assert!(out.tinted_sprites.iter().all(|v| v.base.center.iter().all(|n| n.is_finite())));
}

#[test]
fn mixed_native_frame_resolves_every_emitted_material_and_texture() {
    let mut mesh = bound_mesh();
    let items = [RenderItem::Row(sprite_row("star", "red", "", 0.0, 1.0)), RenderItem::Row(beam_row(1.0, true))];
    let draws = mesh.build(&items).unwrap().draws.clone();
    for draw in draws {
        let material = mesh.profile().materials().get(draw.material.0 as usize).expect("material id");
        assert_eq!(material.layout, draw.source.layout());
        assert!(mesh.profile().textures().get(material.texture.0 as usize).is_some());
    }
}

#[test]
fn warmed_build_reuses_all_hot_storage() {
    let mut mesh = bound_mesh();
    let items = [RenderItem::Batch(sprite_batch()), RenderItem::Row(beam_row(1.0, true))];
    mesh.build(&items).unwrap();
    ALLOCS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    mesh.build(&items).unwrap();
    COUNTING.with(|on| on.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);

    let fallback_items = [RenderItem::Row(sprite_row("unknown", "unknown", "x", 0.0, 1.0))];
    mesh.build(&fallback_items).unwrap();
    ALLOCS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    mesh.build(&fallback_items).unwrap();
    COUNTING.with(|on| on.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0, "resolved fallbacks must also be allocation-free");
}
