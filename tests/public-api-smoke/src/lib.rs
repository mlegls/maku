#[cfg(test)]
mod tests {
    use maku::host::{Inputs, Instance};
    use maku::render::RenderItem;
    use maku::touhou::{TouhouMesh, TouhouProfile};
    use std::rc::Rc;

    #[test]
    fn documented_language_cards_load_through_the_supported_host() {
        const REFERENCE: &str = include_str!("../../../cards/docs/language-reference.maku");
        for pattern in ["reference-ring", "reference-low-level", "reference-render-kind"] {
            let mut instance = Instance::new(None);
            instance.set_render_kinds(["sprite", "beam", "orb"]);
            instance.add_file("language-reference.maku", REFERENCE);
            instance.boot("language-reference.maku".into(), Some(pattern.into()));
            assert!(instance.running(), "{pattern}: {}", instance.status());
            instance.advance(Inputs::default());
            assert!(instance.running(), "{pattern}: {}", instance.status());
        }
    }

    #[test]
    fn supported_facade_builds_a_touhou_frame() {
        let mut instance = Instance::new(None);
        instance.set_render_kinds(TouhouMesh::RENDER_KINDS.iter().copied());
        instance.add_file(
            "smoke.maku",
            r#"
(import "touhou")
(defpattern smoke []
  (par
    (bullet (pose c[1 2]) {:style {:family :orb :color :red}})
    (laser ((pose c[0 1]) ((rot -90) (curve {:u-max 2})))
           {:warn 1 :active 2 :style {:family :laser :color :blue}})))
"#,
        );
        instance.boot("smoke.maku".into(), Some("smoke".into()));
        let mut inputs = Inputs::default();
        inputs.set_vec2("player", 1.0, 2.0);
        for _ in 0..3 {
            instance.advance(inputs.clone());
        }
        assert!(instance.running(), "{}", instance.status());
        assert_eq!(instance.channel_point("player"), Some((1.0, 2.0)));

        let mut pack = TouhouMesh::new(Rc::new(TouhouProfile::stock()));
        for kind in TouhouMesh::RENDER_KINDS {
            if let Some(schema) = instance.declared_render_schema(kind) {
                pack.bind_schema(kind, schema).unwrap();
            }
        }
        let frame = instance.render_frame();
        assert!(
            frame.iter().any(|item| matches!(item, RenderItem::Row(_) | RenderItem::Batch(_))),
            "{}; entities={}", instance.status(), instance.entity_count()
        );
        let built = pack.build(&frame).unwrap();
        assert!(!built.draws.is_empty());
        assert!(!built.vertices.is_empty());
        assert!(built.basic_sprites.len() + built.tinted_sprites.len() + built.recolor_sprites.len() > 0);

        let mut unsupported = Instance::new(None);
        unsupported.set_render_kinds(["default"]);
        unsupported.add_file("smoke.maku", "(import \"touhou\")\n(defpattern p [] (bullet (pose c[0 0])))");
        unsupported.boot("smoke.maku".into(), Some("p".into()));
        assert!(!unsupported.running());
        assert!(
            unsupported.status().contains("render kind"),
            "{}", unsupported.status()
        );
    }
}
