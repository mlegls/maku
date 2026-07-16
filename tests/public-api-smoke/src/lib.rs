#[cfg(test)]
mod tests {
    use maku::host::{Inputs, Instance};
    use maku::render::RenderItem;
    use maku_mesh_touhou::{TouhouMesh, TouhouProfile};
    use std::rc::Rc;

    #[test]
    fn supported_facade_builds_a_touhou_frame() {
        let mut instance = Instance::new(None);
        instance.set_render_kinds(TouhouMesh::RENDER_KINDS.iter().copied());
        instance.add_file(
            "smoke.maku",
            r#"
(import "touhou")
(defpattern smoke []
  (bullet (pose c[1 2]) {:style {:family :orb :color :red}}))
"#,
        );
        instance.boot("smoke.maku".into(), Some("smoke".into()));
        instance.advance(Inputs::default());
        assert!(instance.running(), "{}", instance.status());

        let mut pack = TouhouMesh::new(Rc::new(TouhouProfile::stock()));
        for kind in TouhouMesh::RENDER_KINDS {
            if let Some(schema) = instance.declared_render_schema(kind) {
                pack.bind_schema(kind, schema).unwrap();
            }
        }
        let frame = instance.render_frame();
        assert!(frame.iter().any(|item| matches!(item, RenderItem::Row(_) | RenderItem::Batch(_))));
        assert!(!pack.build(&frame).unwrap().draws.is_empty());
    }
}
