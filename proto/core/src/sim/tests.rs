    use super::*;

    fn live_count(sim: &Sim) -> usize {
        sim.world
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| sim.world.entities.is_alive(*i))
            .count()
    }

    fn live_family_count(sim: &Sim, family: &str) -> usize {
        sim.world
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                sim.world.entities.is_alive(*i)
                    && sim.world.sym_field_matches_at(*i, "family", family)
            })
            .count()
    }

    struct TestStyle {
        family: String,
        color: String,
        variant: String,
    }

    fn style(sim: &Sim, row: usize) -> TestStyle {
        TestStyle {
            family: sim.world.sym_field_resolved_at(row, "family").unwrap_or("").to_string(),
            color: sim.world.sym_field_resolved_at(row, "color").unwrap_or("").to_string(),
            variant: sim.world.sym_field_resolved_at(row, "variant").unwrap_or("").to_string(),
        }
    }

    fn dyn_figure(sim: &Sim, row: usize) -> &DynFigure {
        sim.world.entities.dyn_figure(row).unwrap()
    }

    fn assert_render_rows_eq(a: &RenderRow, b: &RenderRow) {
        match (&a.data, &b.data) {
            (
                RenderData::Point { x: ax, y: ay, theta: at, scale: ascale, alpha: aa, hue: ah },
                RenderData::Point { x: bx, y: by, theta: bt, scale: bscale, alpha: ba, hue: bh },
            ) => {
                assert!((*ax - *bx).abs() < 1e-9, "x: {ax} != {bx}");
                assert!((*ay - *by).abs() < 1e-9, "y: {ay} != {by}");
                assert!((*at - *bt).abs() < 1e-9, "theta: {at} != {bt}");
                assert!((*ascale - *bscale).abs() < 1e-9, "scale: {ascale} != {bscale}");
                assert!((*aa - *ba).abs() < 1e-9, "alpha: {aa} != {ba}");
                assert!((*ah - *bh).abs() < 1e-9, "hue: {ah} != {bh}");
            }
            (
                RenderData::Polyline { points: ap, active: aa },
                RenderData::Polyline { points: bp, active: ba },
            ) => {
                assert_eq!(ap, bp);
                assert_eq!(aa, ba);
            }
            (RenderData::None, RenderData::None) => {}
            _ => panic!("render data mismatch: {:?} != {:?}", a.data, b.data),
        }
        assert_eq!(a.nums, b.nums);
        assert_eq!(a.syms, b.syms);
    }

    fn eval_with_card(card_src: &str, expr_src: &str) -> Val {
        let expanded = crate::edn::expand_src(card_src).unwrap();
        let forms = crate::edn::read_all(&expanded).unwrap();
        let card = load_card(&forms).unwrap();
        let mut ctx = Ctx::default();
        ctx.sig.defs = Rc::new(card.defs.clone());
        ctx.patterns = Rc::new(card.patterns.clone());
        ctx.macros = Rc::new(card.macros.clone());
        let expr = crate::edn::read_one(expr_src).unwrap();
        evaluate(&expr, &Env::empty(), &mut ctx, &mut World::default()).unwrap()
    }

    fn loaded_def(card_src: &str, name: &str) -> Form {
        let expanded = crate::edn::expand_src(card_src).unwrap();
        let forms = crate::edn::read_all(&expanded).unwrap();
        let card = load_card(&forms).unwrap();
        card.defs.get(name).unwrap().clone()
    }

    fn form_contains_head(form: &Form, head: &str) -> bool {
        match form {
            Form::List(items) => {
                matches!(items.first(), Some(Form::Sym(s)) if s.as_ref() == head)
                    || items.iter().any(|f| form_contains_head(f, head))
            }
            Form::Vector(items) => items.iter().any(|f| form_contains_head(f, head)),
            Form::Map(kvs) => kvs
                .iter()
                .any(|(k, v)| form_contains_head(k, head) || form_contains_head(v, head)),
            _ => false,
        }
    }

    #[test]
    fn rewrite_value_or_call_preserves_results() {
        let card = "(defn use-value-or [x d] (value-or x d))";
        let rewritten = loaded_def(card, "use-value-or");
        assert!(form_contains_head(&rewritten, VALUE_OR_INTRINSIC), "not rewritten: {}", rewritten);

        assert!(matches!(eval_with_card(card, "(use-value-or (channel $missing) 7)"), Val::Num(n) if n == 7.0));
        assert!(matches!(eval_with_card(card, "(use-value-or 3 7)"), Val::Num(n) if n == 3.0));
    }

    #[test]
    fn rewrite_inlines_wrappers_transitively() {
        // col-or's shape: a trivial wrapper around a defn that only becomes
        // trivial after its own body is rewritten — needs the fixpoint
        let card = "(defn base [x d] (if (nothing? x) d x))\n\
                    (defn wrap [x d] (base x d))\n\
                    (defn use-wrap [x] (wrap x 5))";
        let rewritten = loaded_def(card, "use-wrap");
        assert!(form_contains_head(&rewritten, VALUE_OR_INTRINSIC), "wrapper not inlined transitively: {}", rewritten);
        assert!(!form_contains_head(&rewritten, "wrap"), "wrap call survived: {}", rewritten);

        assert!(matches!(eval_with_card(card, "(use-wrap (channel $missing))"), Val::Num(n) if n == 5.0));
        assert!(matches!(eval_with_card(card, "(use-wrap 3)"), Val::Num(n) if n == 3.0));
    }

    #[test]
    fn rewrite_hand_written_value_or_shape_matches_lib_route() {
        let card = "(defn hand [x d] (if (nothing? x) d x))";
        let rewritten = loaded_def(card, "hand");
        assert!(form_contains_head(&rewritten, VALUE_OR_INTRINSIC), "not rewritten: {}", rewritten);

        assert!(matches!(eval_with_card(card, "(hand (channel $missing) 11)"), Val::Num(n) if n == 11.0));
        assert!(matches!(eval_with_card(card, "(hand 5 11)"), Val::Num(n) if n == 5.0));
    }

    #[test]
    fn rewrite_value_or_shape_rejects_impure_x() {
        let rewritten = loaded_def(
            "(defn impure [] (if (nothing? (rand 0 1)) 9 (rand 0 1)))",
            "impure",
        );
        assert!(!form_contains_head(&rewritten, VALUE_OR_INTRINSIC), "impure form rewritten: {}", rewritten);
        assert!(form_contains_head(&rewritten, "if"), "impure form no longer has original if: {}", rewritten);
    }

    #[test]
    fn rewrite_respects_shadowed_names() {
        let shadow_nothing = "(defn shadow-nothing [x]\n  ((fn [nothing?] (if (nothing? x) 1 x)) (fn [v] 0)))";
        let rewritten = loaded_def(shadow_nothing, "shadow-nothing");
        assert!(!form_contains_head(&rewritten, VALUE_OR_INTRINSIC), "shadowed nothing? rewritten: {}", rewritten);
        assert!(matches!(eval_with_card(shadow_nothing, "(shadow-nothing (channel $missing))"), Val::Nothing));

        let shadow_value_or = "(defn shadow-value-or []\n  (let [value-or (fn [x d] d)] (value-or 3 9)))";
        let rewritten = loaded_def(shadow_value_or, "shadow-value-or");
        assert!(!form_contains_head(&rewritten, VALUE_OR_INTRINSIC), "shadowed value-or inlined: {}", rewritten);
        assert!(matches!(eval_with_card(shadow_value_or, "(shadow-value-or)"), Val::Num(n) if n == 9.0));

        let top_level_nothing = "(defn nothing? [x] 0)\n(defn top-level [x] (if (nothing? x) 1 x))";
        let rewritten = loaded_def(top_level_nothing, "top-level");
        assert!(!form_contains_head(&rewritten, VALUE_OR_INTRINSIC), "top-level shadowed nothing? rewritten: {}", rewritten);
        assert!(matches!(eval_with_card(top_level_nothing, "(top-level (channel $missing))"), Val::Nothing));
    }

    /// Conformance: the real translation files, loaded verbatim from disk.
    #[test]
    #[ignore = "long corpus test; run with cargo test --lib -- --ignored --test-threads=1"]
    fn translations_run() {
        let cases: &[(&str, &str, usize)] = &[
            ("../../cards/translations/130_bowap.maku", "bowap", 300),
            ("../../cards/translations/130_bowap.maku", "bowap-fold", 300),
            ("../../cards/translations/020_gsrepeat.maku", "gsrepeat-demo", 300),
            ("../../cards/translations/040_spread.maku", "spread-demo", 300),
            ("../../cards/translations/060_polar.maku", "polar-demo", 300),
            ("../../cards/translations/080_aimed.maku", "aimed-demo", 400),
            ("../../cards/translations/070_dynamic_lasers.maku", "lasers-demo", 300),
            ("../../cards/translations/110_exploding_stars.maku", "exploding-stars", 400),
            ("../../cards/translations/200_cradle.maku", "cradle", 300),
            ("../../cards/translations/player_homing.maku", "reimu-free-fire", 300),
            ("../../cards/translations/player_homing.maku", "reimu-focus", 400),
            ("../../cards/translations/player_homing.maku", "fantasy-seal", 700),
            ("../../cards/translations/ph_boss2_spell2.maku", "spell-2", 900),
        ];
        for (path, pattern, ticks) in cases {
            let src = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("{}: {}", path, e));
            let mut sim = Sim::load(&src, Some(pattern))
                .unwrap_or_else(|e| panic!("{} [{}]: {}", path, pattern, e));
            for _ in 0..*ticks {
                sim.step()
                    .unwrap_or_else(|e| panic!("{} [{}]: {}", path, pattern, e));
            }
            assert!(
                !sim.world.entities.is_empty(),
                "{} [{}]: no entities after {} ticks",
                path,
                pattern,
                ticks
            );
        }
    }

    const BOWAP: &str = r#"
(defpattern bowap [speed 4.0
                   arms  5
                   period (ticks 8)]
  ((pose c[0 2])
    (dotimes [i inf :every period]
      (spawn ((rot (* 0.2 (+ i 1) (+ i 2)))
              (circle arms (linear c[speed 0])))
             {:style {:family :gem :variant :w
                      :color [:yellow :orange :red :pink :purple]}}))))
"#;

    #[test]
    fn bowap_headless() {
        let mut sim = Sim::load(BOWAP, Some("bowap")).unwrap();
        for _ in 0..120 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.entities.len(), 15 * 5, "15 volleys × 5 arms");

        let sig = SigEnv::default();
        assert_eq!(sim.world.entities.birth(0), Some(0));
        assert_eq!(style(&sim, 0).family, "gem");
        assert_eq!(style(&sim, 0).color, "yellow");
        let state = MotionState::new();
        let p = dyn_figure_pose(dyn_figure(&sim, 0), 1.0, &state, &sig).unwrap();
        let ang = (0.4f64).to_radians();
        assert!((p.x - 4.0 * ang.cos()).abs() < 1e-9, "x: {}", p.x);
        assert!((p.y - (2.0 + 4.0 * ang.sin())).abs() < 1e-9, "y: {}", p.y);

        assert_eq!(style(&sim, 1).color, "orange");
        assert_eq!(style(&sim, 4).color, "purple");

        assert_eq!(sim.world.entities.birth(5), Some(8));
    }

    #[test]
    fn bowap_fold_version_matches() {
        const BOWAP_B: &str = r#"
(defpattern bowap-fold [speed 4.0
                        arms  5
                        period (ticks 8)]
  ((pose c[0 2])
    (loop [increment 0.4
           base      0.4]
      (spawn ((rot base)
               (circle arms (linear c[speed 0])))
             {:style {:family :gem :variant :w
                      :color [:yellow :orange :red :pink :purple]}})
      (wait period)
      (recur (+ increment 0.4)
             (+ base increment 0.4)))))
"#;
        let mut sa = Sim::load(BOWAP, Some("bowap")).unwrap();
        let mut sb = Sim::load(BOWAP_B, Some("bowap-fold")).unwrap();
        for _ in 0..240 {
            sa.step().unwrap();
            sb.step().unwrap();
        }
        assert_eq!(sa.world.entities.len(), sb.world.entities.len());
        let sig = SigEnv::default();
        for (i, _) in sa.world.entities.iter().zip(sb.world.entities.iter()).enumerate() {
            assert_eq!(sa.world.entities.birth(i), sb.world.entities.birth(i));
            let state = MotionState::new();
            let pa = dyn_figure_pose(dyn_figure(&sa, i), 0.7, &state, &sig).unwrap();
            let pb = dyn_figure_pose(dyn_figure(&sb, i), 0.7, &state, &sig).unwrap();
            assert!(
                (pa.x - pb.x).abs() < 1e-6 && (pa.y - pb.y).abs() < 1e-6,
                "A/B diverged: {:?} vs {:?}",
                pa,
                pb
            );
        }
    }

    /// 110's mechanism end-to-end: let-bound spawn handles + scheduled
    /// manip with explode-and-cull.
    #[test]
    fn handles_and_manip() {
        const CARD: &str = r#"
(defpattern boom []
  ((pose c[0 1])
    (let [stars (spawn (circle 4 (linear c[1 0])) {:style {:family :lstar}})]
      (seq
        (wait 0.5)
        (manip (nth stars 0)
          (fn [b]
            (spawn (+ (pos b) (circle 8 (linear c[2 0])))
                   {:style {:family :star}})
            (cull b :soft)))))))
"#;
        let mut sim = Sim::load(CARD, Some("boom")).unwrap();
        for _ in 0..120 {
            sim.step().unwrap();
        }
        let lstars = live_family_count(&sim, "lstar");
        let stars = live_family_count(&sim, "star");
        assert_eq!(lstars, 3, "one big star culled");
        assert_eq!(stars, 8, "explosion ring spawned");
        // ring anchored at the culled star's position at t≈0.5 (x = 0.5 from
        // anchor (0,1)); fn bodies drop the ambient frame, so no double anchor
        let sig = SigEnv::default();
        let ring: Vec<_> = sim
            .world
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| style(&sim, *i).family == "star")
            .map(|(i, _)| i)
            .collect();
        let state = MotionState::new();
        let p = dyn_figure_pose(dyn_figure(&sim, ring[0]), 0.0, &state, &sig).unwrap();
        assert!((p.x - 0.5).abs() < 0.02 && (p.y - 1.0).abs() < 0.02, "ring anchor: {:?}", p);
    }

    /// Snapshot determinism: clone mid-run, step both with identical inputs,
    /// worlds stay identical (the scrubbing contract).
    #[test]
    fn snapshot_determinism() {
        let src = std::fs::read_to_string("../../cards/translations/ph_boss2_spell2.maku").unwrap();
        let mut a = Sim::load(&src, Some("spell-2")).unwrap();
        for _ in 0..200 {
            a.step().unwrap();
        }
        let mut b = a.clone();
        let inputs = Inputs::classic((1.5, -3.0), (-2.0, 2.0));
        for _ in 0..300 {
            a.step_with(&inputs).unwrap();
            b.step_with(&inputs).unwrap();
        }
        assert_eq!(a.world.entities.len(), b.world.entities.len());
        for (i, _) in a.world.entities.iter().zip(b.world.entities.iter()).enumerate() {
            assert_eq!(a.world.entities.generation(i), b.world.entities.generation(i));
            assert_eq!(a.world.entities.is_alive(i), b.world.entities.is_alive(i));
            let tau = a.world.entity_tau(i, a.world.tick);
            let state = MotionState::new();
            let ax = a.motion_readers(i);
            let by = b.motion_readers(i);
            let px = dyn_figure_pose_in(dyn_figure(&a, i), tau, MotionEvalCtx::new(&state, &a.ctx.sig, &ax)).unwrap();
            let py = dyn_figure_pose_in(dyn_figure(&b, i), tau, MotionEvalCtx::new(&state, &b.ctx.sig, &by)).unwrap();
            assert!(
                (px.x - py.x).abs() < 1e-12 && (px.y - py.y).abs() < 1e-12,
                "diverged: {:?} vs {:?}",
                px,
                py
            );
        }
    }

    /// Hostile fire hits the tiny player hitbox once, then iframes absorb
    /// the follow-up; the bullet that hit is culled.
    #[test]
    fn player_hit_and_iframes() {
        // two entities aimed straight down the player's column, 10 ticks apart
        const CARD: &str = r#"
(import "touhou")
(defpattern rig []
  (let [p (spawn (live $player)
                 (circle-collider {:layer :player-hurt :r 0.06})
                 {:team :player-body
                  :lives 3 :graze 0 :hits 0})]
    (let [body (nth p 0)]
      (bind-channel! $graze (:graze body))
      (bind-channel! $hits (:hits body)))))
(defpattern atk []
  (par (rig)
    (dotimes [i 2 :every (ticks 10)]
      (bullet (in-frame (pose c[0 3]) (vel c[0 -6])) {}))))
"#;
        let mut sim = Sim::load(CARD, Some("atk")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..120 {
            sim.step_with(&inputs).unwrap();
        }
        assert!(matches!(sim.channel_val("hits"), Some(Val::Num(n)) if n == 1.0), "second bullet fell in iframes");
        let hits: Vec<_> =
            sim.events_vec().into_iter().filter(|e| &*e.name == "player-hit").collect();
        assert_eq!(hits.len(), 1);
        // the iframed bullet passed through (grazing) and is still flying
        assert_eq!(
            sim.world.entities.iter().enumerate().filter(|(i, _)| {
                sim.world.entities.is_alive(*i) && sim.world.sym_field_missing_at(*i, "team")
            }).count(),
            1
        );
        assert!(matches!(sim.channel_val("graze"), Some(Val::Num(n)) if n == 2.0), "graze ring precedes the hitbox; iframes graze too");
        // the hit effect is a column write; $lives is a channel
        assert!(matches!(sim.channel_val("lives"), Some(Val::Num(n)) if n == 2.0));
    }

    /// A bullet passing beside the player grazes exactly once.
    #[test]
    fn graze_counts_once() {
        const CARD: &str = r#"
(import "touhou")
(defpattern rig []
  (let [p (spawn (live $player)
                 (circle-collider {:layer :player-hurt :r 0.06})
                 {:team :player-body
                  :graze 0 :hits 0})]
    (let [body (nth p 0)]
      (bind-channel! $graze (:graze body))
      (bind-channel! $hits (:hits body)))))
(defpattern g []
  (par (rig) (bullet (in-frame (pose c[0.25 3]) (vel c[0 -6])) {})))
"#;
        let mut sim = Sim::load(CARD, Some("g")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..120 {
            sim.step_with(&inputs).unwrap();
        }
        assert!(matches!(sim.channel_val("hits"), Some(Val::Num(n)) if n == 0.0), "0.25 off-axis misses the 0.06 hitbox");
        assert!(matches!(sim.channel_val("graze"), Some(Val::Num(n)) if n == 1.0), "graze latches once per bullet");
        // and the counter is a channel patterns can read
        assert!(matches!(sim.channel_val("graze"), Some(Val::Num(n)) if n == 1.0));
    }

    #[test]
    fn deftick_collision_custom_rule() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]] (seq (event :zapped (:pos b)) (cull a)))
       (collisions :zap :zappable)))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) (circle-collider {:layer :zap :r 0.2}))
    (spawn (pose c[0 0]) (circle-collider {:layer :zappable :r 0.2}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        for _ in 0..3 {
            sim.step().unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "zapped").count(), 1);
    }

    #[test]
    fn circle_collider_constructor_projects() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]] (seq (event :zapped (:pos b)) (cull a)))
       (collisions :zap :zappable)))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) (circle-collider {:layer :zap :r 0.2}))
    (spawn (pose c[0 0]) (circle-collider {:layer :zappable :r 0.2}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        for _ in 0..3 {
            sim.step().unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "zapped").count(), 1);
    }

    #[test]
    fn spawn_collider_slot_list_yields_both_rows() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]] (seq (event :zapped (:pos b)) (cull a)))
       (collisions :zap :zappable)))
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:grazed a)) 0 (:grazed a)) 1)
           (seq (set-col a :grazed 1) (event :grazed (:pos b)))))
       (collisions :graze :zappable)))
(defpattern t []
  (seq
    (spawn (pose c[0 0])
      [
        (circle-collider {:layer :zap :r 0.05})
        (circle-collider {:layer :graze :r 0.3})])
    (spawn (pose c[0.2 0]) (circle-collider {:layer :zappable :r 0.05}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        for _ in 0..3 {
            sim.step().unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "zapped").count(), 0);
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "grazed").count(), 1);
    }

    #[test]
    fn defcollider_registers_named_projector() {
        const CARD: &str = r#"
(defcollider bullet-collider [e ctx]
  [
    (circle-collider {:layer :zap :r 0.05})
    (circle-collider {:layer :graze :r 0.3})])
(deftick
  (map (fn [[a b]] (seq (event :zapped (:pos b)) (cull a)))
       (collisions :zap :zappable)))
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:grazed a)) 0 (:grazed a)) 1)
           (seq (set-col a :grazed 1) (event :grazed (:pos b)))))
       (collisions :graze :zappable)))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) bullet-collider)
    (spawn (pose c[0.2 0]) (circle-collider {:layer :zappable :r 0.05}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        for _ in 0..3 {
            sim.step().unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "zapped").count(), 0);
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "grazed").count(), 1);
    }

    #[test]
    fn defcollider_body_empty_list_yields_no_colliders() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]] (event :hit (:pos b)))
       (collisions :zap :zappable)))
(defcollider empty-collider [e ctx] [])
(defpattern t []
  (seq
    (spawn (pose c[0 0]) empty-collider)
    (spawn (pose c[0 0]) (circle-collider {:layer :zappable :r 0.2}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "hit").count(), 0);
    }

    #[test]
    fn defcollider_body_nothing_yields_no_colliders() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]] (event :hit (:pos b)))
       (collisions :zap :zappable)))
(defcollider empty-collider [e ctx]
  (if (< 1 0) (circle-collider {:layer :zap :r 0.2})))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) empty-collider)
    (spawn (pose c[0 0]) (circle-collider {:layer :zappable :r 0.2}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "hit").count(), 0);
    }

    #[test]
    fn collider_body_nested_list_errors() {
        const CARD: &str = r#"
(defcollider bad-collider [e ctx]
  [[(circle-collider {:layer :zap :r 0.2})]
   (circle-collider {:layer :graze :r 0.2})])
(defpattern t []
  (spawn (pose c[0 0]) bad-collider))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        let err = sim.step().unwrap_err();
        assert!(
            err.contains("collider: expected collider projector or list of them"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn inline_anonymous_collider_in_spawn_argument_works() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]] (event :hit (:pos b)))
       (collisions :zap :zappable)))
(defpattern t []
  (seq
    (spawn (pose c[0 0])
      (collider :pose [e ctx]
        (circle-collider {:layer :zap :r 0.2})))
    (spawn (pose c[0 0]) (circle-collider {:layer :zappable :r 0.2}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "hit").count(), 1);
    }

    #[test]
    fn defcollider_accepts_explicit_pose_figure_type() {
        const CARD: &str = r#"
(defcollider :pose hitbox-collider [e ctx]
  (circle-collider {:layer :damage :r e.hitbox}))
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit (:pos b)))))
       (collisions :damage :body)))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) {:hitbox 0.3} hitbox-collider)
    (spawn (pose c[0.35 0]) (circle-collider {:layer :body :r 0.1}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "hit").count(), 1);
    }

    #[test]
    fn defcollider_accepts_parametric_figure_type() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit))))
       (collisions :beam :body)))
(defcollider :parametric laser-collider [e ctx]
  (capsule-chain-collider {:layer :beam :r 0.1}))
(defpattern t []
  (par
    (spawn (pose c[0 0]) (circle-collider {:layer :body :r 0.06}))
    (spawn ((pose c[-2 0]) (curve {:u-max 6}))
           laser-collider)))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "hit").count(), 1);
    }

    #[test]
    fn defcollider_rejects_unknown_figure_type() {
        const CARD: &str = r#"
(defcollider :polyline laser-collider [e ctx]
  (capsule-chain-collider {:layer :beam :r 0.1}))
(defpattern t [] (spawn (pose c[0 0]) laser-collider))
"#;
        let Err(err) = Sim::load(CARD, Some("t")) else {
            assert!(false, "unknown defcollider figure type unexpectedly loaded");
            return;
        };
        assert!(
            err.contains("unsupported figure type :polyline"),
            "expected unsupported figure type error, got {err}"
        );
    }

    #[test]
    fn defcollider_can_read_entity_meta() {
        const CARD: &str = r#"
(defcollider hitbox-collider [entity context]
  (circle-collider {:layer :damage :r entity.hitbox}))
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit (:pos b)))))
       (collisions :damage :body)))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) {:hitbox 0.3} hitbox-collider)
    (spawn (pose c[0.35 0]) (circle-collider {:layer :body :r 0.1}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "hit").count(), 1);
    }

    #[test]
    fn defcollider_reads_top_level_numeric_meta() {
        const CARD: &str = r#"
(defcollider hitbox-collider [entity context]
  (circle-collider {:layer :damage :r entity.hitbox}))
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit (:pos b)))))
       (collisions :damage :body)))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) {:hitbox 0.3} hitbox-collider)
    (spawn (pose c[0.35 0]) (circle-collider {:layer :body :r 0.1}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "hit").count(), 1);
    }

    #[test]
    fn collider_overrides_do_not_treat_keywords_as_entity_fields() {
        const CARD: &str = r#"
(defcollider hitbox-collider [e ctx]
  (circle-collider {:layer :damage :r :hitbox}))
(defpattern t []
  (spawn (pose c[0 0]) {:hitbox 0.3} hitbox-collider))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        let err = match sim.step() {
            Ok(_) => panic!("keyword radius override unexpectedly stepped"),
            Err(err) => err,
        };
        assert!(
            err.contains("expected number"),
            "keyword radius override should be a type error, got {err}"
        );
    }

    #[test]
    fn defcollider_requires_entity_and_context_params() {
        let err = match Sim::load(
            r#"
(defcollider bad [e] (circle-collider {:layer :zap :r 0.1}))
(defpattern t [] (spawn (pose c[0 0]) (bad)))
"#,
            Some("t"),
        ) {
            Ok(_) => panic!("bad defcollider unexpectedly loaded"),
            Err(err) => err,
        };
        assert!(err.contains("defcollider: expected two parameters"), "{err}");
    }

    #[test]
    fn deftick_maps_entity_domains() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (if (<= (:hp e) 0)
           (seq (event :died (:pos e)) (cull e))))
       (entities-where {:team :enemy})))
(defpattern p []
  (spawn (pose c[0 0]) {:team :enemy :hp 0}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert!(sim.events_vec().iter().any(|e| &*e.name == "died"));
        assert_eq!(sim.world.entities.iter().enumerate().filter(|(i, _)| sim.world.entities.is_alive(*i)).count(), 0);
    }

    #[test]
    fn deftick_maps_collision_domains() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]] (seq (event :zapped (:pos b)) (cull a)))
       (collisions :zap :body)))
(defpattern p []
  (par
    (spawn (pose c[0 0]) (circle-collider {:layer :zap :r 0.2}))
    (spawn (pose c[0 0]) (circle-collider {:layer :body :r 0.2}) {:team :target})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert!(sim.events_vec().iter().any(|e| &*e.name == "zapped"));
        assert_eq!(sim.world.entities.iter().enumerate().filter(|(i, _)| sim.world.entities.is_alive(*i)).count(), 1);
    }

    #[test]
    fn collision_domain_latch_with_field() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:latched a)) 0 (:latched a)) 1)
           (seq (set-col a :latched 1) (event :zapped (:pos a)))))
       (collisions :zap :zappable)))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) (circle-collider {:layer :zap :r 0.2}))
    (spawn (pose c[0 0]) (circle-collider {:layer :zappable :r 0.2}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        for _ in 0..5 {
            sim.step().unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "zapped").count(), 1);
        assert!(sim
            .world
            .entities
            .iter()
            .enumerate()
            .any(|(i, _)| sim.world.col_get_at(i, "latched") == Some(1.0)));
    }

    #[test]
    fn collision_domain_filter_predicate() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]]
         (if (<= (if (nothing? (:shield b)) 0 (:shield b)) 0)
           (event :zapped (:pos b))))
       (collisions :zap :zappable)))
(defpattern t []
  (let [a (spawn (pose c[0 0]) (circle-collider {:layer :zap :r 0.2}))
        b (spawn (pose c[0 0]) {:shield 1}
                                (circle-collider {:layer :zappable :r 0.2}))]
    (seq (wait 0.05) (set-col (first b) :shield 0))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        for _ in 0..3 {
            sim.step().unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "zapped").count(), 0);
        for _ in 0..10 {
            sim.step().unwrap();
        }
        assert!(sim.events_vec().iter().any(|e| &*e.name == "zapped"));
    }

    #[test]
    fn multiple_deftick_rules_compose() {
        const CARD: &str = r#"
(deftick (map (fn [[a b]] (event :first (:pos b))) (collisions :zap :zappable)))
(deftick (map (fn [[a b]] (event :second (:pos b))) (collisions :zap :zappable)))
(defpattern t []
  (seq
    (spawn (pose c[0 0]) (circle-collider {:layer :zap :r 0.2}))
    (spawn (pose c[0 0]) (circle-collider {:layer :zappable :r 0.2}))))
"#;
        let mut sim = Sim::load(CARD, Some("t")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "first").count(), 1);
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "second").count(), 1);
    }

    /// Player fire decrements :hp; at zero the enemy dies with an event and
    /// the $enemies channel reflects it.
    #[test]
    fn enemy_hp_and_death() {
        const CARD: &str = r#"
(import "touhou")
(defpattern duel []
  (seq
    (enemy (pose c[0 2]) {:hp 2 :hitbox 0.3})
    (dotimes [i 3 :every (ticks 30)]
      (shot (in-frame (pose c[0 0]) (vel c[0 4]))
                  {:damage 1}))))
"#;
        let mut sim = Sim::load(CARD, Some("duel")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        // shot 1 (fired tick 0, 4 u/s) reaches the enemy ring at ~tick 47
        for _ in 0..55 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "enemy-hit").count(), 1);
        assert!(matches!(sim.channel_val("enemies"), Some(Val::Num(n)) if n == 1.0));
        // shot 2 kills at ~tick 77; shot 3 flies through empty space
        for _ in 0..55 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "died").count(), 1);
        assert!(matches!(sim.channel_val("enemies"), Some(Val::Num(n)) if n == 0.0));
    }

    /// The gameplay layer lives in World, so it scrubs: rewind to before a
    /// graze and the counter rewinds with it; re-step and it recurs.
    #[test]
    fn gameplay_scrubs() {
        use crate::session::Session;
        const CARD: &str = r#"
(import "touhou")
(defpattern g [] (bullet (in-frame (pose c[0.25 3]) (vel c[0 -6])) {}))
"#;
        let mut sess = Session::default();
        sess.rig = Some(
            "(defpattern rig [] (let [p (spawn (live $player) (circle-collider {:layer :player-hurt :r 0.06}) {:team :player-body \
             :graze 0 :hits 0})] (let [body (nth p 0)] (bind-channel! $graze (:graze body)) (bind-channel! $hits (:hits body)))))"
                .into(),
        );
        sess.last_inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        sess.start(Sim::load(CARD, Some("g")).unwrap());
        for _ in 0..120 {
            sess.advance(CARD).unwrap();
        }
        assert_eq!(sess.sim.as_ref().unwrap().channel_u64("graze"), 1);
        sess.seek(CARD, 10).unwrap();
        assert_eq!(sess.sim.as_ref().unwrap().channel_u64("graze"), 0, "rewound past the graze");
        sess.seek(CARD, 120).unwrap();
        let sim = sess.sim.as_ref().unwrap();
        assert_eq!(sim.channel_u64("graze"), 1, "replay re-grazes, not double-counts");
        assert_eq!(
            sim.events_vec().iter().filter(|e| &*e.name == "graze").count(),
            1,
            "the shared log was truncated at restore and re-populated"
        );
    }

    /// The player is an ordinary entity: lives is a column decremented by
    /// the hit effect; game-over is its (non-culling) trigger.
    #[test]
    fn lives_and_game_over() {
        const CARD: &str = r#"
(import "touhou")
(defpattern rig []
  (let [p (spawn (live $player)
                 (circle-collider {:layer :player-hurt :r 0.06})
                 {:team :player-body
                  :lives 2 :graze 0 :hits 0})]
    (let [body (nth p 0)]
      (bind-channel! $graze (:graze body))
      (bind-channel! $hits (:hits body)))))
(defpattern atk []
  (par (rig)
    (dotimes [i 5 :every (ticks 70)]
      (bullet (in-frame (pose c[0 3]) (vel c[0 -6])) {}))))
"#;
        let mut sim = Sim::load(CARD, Some("atk")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..300 {
            sim.step_with(&inputs).unwrap();
        }
        // 70-tick cadence clears the 60-tick iframes: all 4 arrivals hit
        let count = |n: &str| sim.events_vec().iter().filter(|e| &*e.name == n).count();
        assert_eq!(count("player-hit"), 4);
        assert_eq!(count("game-over"), 1, "trigger edge-fires once at lives 0, latched");
        // the column keeps counting (what game-over MEANS is host policy)
        assert!(matches!(sim.channel_val("lives"), Some(Val::Num(n)) if n == -2.0));
        // non-culling: the player entity is still there (host decides)
        assert!(sim.world.entities.iter().enumerate().any(|(i, _)| {
            sim.world.entities.is_alive(i) && sim.world.sym_field_matches_at(i, "team", "player-body")
        }));
    }

    /// Death is not special: deftick can gate a phase event at low hp and
    /// Touhou's library rule kills at zero. Latches are ordinary fields.
    #[test]
    fn deftick_thresholds() {
        const CARD: &str = r#"
(import "touhou")
(deftick
  (map (fn [e]
         (seq
           (set-col e :lowhp-fired 1)
           (event :low-hp (:pos e))))
       (entities-where (fn [e] (* (= e.team :enemy)
                                  (<= (col-or (:hp e) 1) 1)
                                  (< (col-or (:lowhp-fired e) 0) 1))))))
(defpattern gates []
  (seq
    (enemy (pose c[0 2]) {:hp 3 :hitbox 0.3})
    (dotimes [i 3 :every (ticks 30)]
      (shot (in-frame (pose c[0 0]) (vel c[0 4]))
                  {:damage 1}))))
"#;
        let mut sim = Sim::load(CARD, Some("gates")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..200 {
            sim.step_with(&inputs).unwrap();
        }
        let count = |n: &str| sim.events_vec().iter().filter(|e| &*e.name == n).count();
        assert_eq!(count("enemy-hit"), 3, "every contact writes the column");
        assert_eq!(count("low-hp"), 1, "gate fired once at hp 1, latched");
        assert_eq!(count("died"), 1, "death is just the second threshold");
        assert!(matches!(sim.channel_val("enemies"), Some(Val::Num(n)) if n == 0.0));
    }

    /// DMK player() damage maps lower their :hit value to the ordinary
    /// numeric :damage column used by Touhou contacts.
    #[test]
    fn damage_map_hit_lowers_to_column() {
        const CARD: &str = r#"
(import "touhou")
(defpattern duel []
  (seq
    (enemy (pose c[0 2]) {:hp 3 :hitbox 0.3})
    (shot (in-frame (pose c[0 0]) (vel c[0 4]))
                {:damage {:hit 4 :graze 9}})))
"#;
        let mut sim = Sim::load(CARD, Some("duel")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..60 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(
            sim.events_vec().iter().filter(|e| &*e.name == "died").count(),
            1,
            "hit damage 4 beats hp 3 in one contact"
        );
    }

    /// Active lasers collide as capsule chains sampled from the same curve
    /// the renderer draws; beams persist through a hit (no cull).
    #[test]
    fn laser_hitbox() {
        const CARD: &str = r#"
(import "touhou")
(defpattern rig []
  (let [p (spawn (live $player)
                 (circle-collider {:layer :player-hurt :r 0.06})
                 {:team :player-body
                  :graze 0 :hits 0})]
    (let [body (nth p 0)]
      (bind-channel! $graze (:graze body))
      (bind-channel! $hits (:hits body)))))
(defpattern beam []
  (par (rig) (laser ((pose c[-2 0]) (curve {:u-max 6})) {:warn 0.5 :active 2})))
"#;
        let mut sim = Sim::load(CARD, Some("beam")).unwrap();
        // player parked ON the beam line, 2 units along it
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        // warn phase: no hitbox
        for _ in 0..50 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.channel_u64("hits"), 0, "warn phase doesn't hit");
        for _ in 0..30 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.channel_u64("hits"), 1, "active beam hits");
        assert_eq!(
            sim.world.entities.iter().enumerate().filter(|(i, _)| {
                sim.world.entities.is_alive(*i) && sim.world.sym_field_missing_at(*i, "team")
            }).count(),
            1,
            "the beam persists through the hit"
        );
        assert_eq!(sim.channel_u64("graze"), 1, "beam grazed on the way in");
    }

    #[test]
    fn low_level_curve_slots() {
        const CARD: &str = r#"
(deftick (map (fn [[a b]] (event :hit)) (collisions :beam :body)))
(defpattern p []
  (par
    (spawn (pose c[0 0])
           (circle-collider {:layer :body :r 0.06}))
    (spawn ((pose c[-2 0]) (curve {:u-max 6}))
           (capsule-chain-collider {:layer :beam :r 0.06 :width 1
                                    :resolution 0.1 :u-max 6})
           {:render :test-beam})))
(deftick
  (map (fn [e]
         (render {:shape (curve-samples e {:resolution 0.1 :u-max 6})
                  :active 1}))
       (entities-where (fn [e] (= e.render :test-beam)))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "hit"),
            "explicit capsule-chain collider hit the circle body"
        );
        assert!(
            sim.render().iter().any(|r| {
                matches!(&r.data, RenderData::Polyline { active, .. } if *active)
            }),
            "explicit polyline renderer produced a render item"
        );
    }

    #[test]
    fn touhou_laser_collider_reads_lifecycle_from_meta() {
        const CARD: &str = r#"
(import "touhou")
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit))))
       (collisions :damage :body)))
(defpattern p []
  (par
    (spawn (pose c[0 0])
           (circle-collider {:layer :body :r 0.06}))
    (spawn ((pose c[-2 0]) (curve {:u-max 6}))
           {:warn 0 :active 2 :u-max 6 :radius 0.06 :resolution 0.1}
           laser-collider)))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "hit"),
            "touhou laser-collider should project an active capsule chain"
        );
    }

    #[test]
    fn deftick_render_emits_point_row() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (render {:shape :point
                  :x (:x (:pos e))
                  :y (:y (:pos e))
                  :theta 30
                  :scale e.scale
                  :alpha 0.75
                  :hue 120}))
       (entities-where {:render :sprite})))
(defpattern p []
  (spawn (pose c[1 2]) {:render :sprite :scale 2}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let items = sim.render();
        let Some(row) = items.first() else {
            panic!("render should emit one point row");
        };
        let RenderData::Point { x, y, theta: th, scale, alpha, hue } = &row.data else {
            panic!("render should emit one point row");
        };
        assert!((*x - 1.0).abs() < 1e-9, "x: {x}");
        assert!((*y - 2.0).abs() < 1e-9, "y: {y}");
        assert!((*th - 30.0).abs() < 1e-9, "th: {th}");
        assert!((*scale - 2.0).abs() < 1e-9, "scale: {scale}");
        assert!((*alpha - 0.75).abs() < 1e-9, "alpha: {alpha}");
        assert!((*hue - 120.0).abs() < 1e-9, "hue: {hue}");
    }

    #[test]
    fn compiled_render_rule_matches_interpreted_rows() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (let [p (:pos e)]
           (render {:shape :point
                    :x (:x p) :y (:y p)
                    :theta (value-or e.facing (:th p))
                    :scale (value-or e.scale 1)
                    :alpha (value-or e.opacity 1)
                    :hue (value-or e.hue 0)
                    :family e.family :color e.color :variant e.variant})))
       (entities-where (fn [e] (* (= e.render :sprite) (= e.kind :point))))))
(defpattern p []
  (par
    (spawn (pose c[1 2]) {:render :sprite :facing 30 :scale 2 :opacity 0.75 :hue 120
                           :family :orb :color :red :variant :large})
    (spawn (pose c[3 4]) {:render :sprite :family :orb :color :blue :variant :small})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        assert!(sim.world.standing_rules[0].compiled[0].is_some());
        sim.step().unwrap();
        let rows = sim.render();
        assert_eq!(rows.len(), 2);
        let RenderData::Point { x, y, theta, scale, alpha, hue } = rows[0].data else { panic!() };
        assert_eq!((x, y, theta, scale, alpha, hue), (1.0, 2.0, 30.0, 2.0, 0.75, 120.0));
        assert_eq!((rows[0].sym("family"), rows[0].sym("color"), rows[0].sym("variant")),
            (Some("orb"), Some("red"), Some("large")));
        let RenderData::Point { x, y, theta, scale, alpha, hue } = rows[1].data else { panic!() };
        assert_eq!((x, y, theta, scale, alpha, hue), (3.0, 4.0, 0.0, 1.0, 1.0, 0.0));
    }

    #[test]
    fn compiled_render_rule_bails_and_macroexpands_at_registration() {
        const CARD: &str = r#"
(defmacro point-row [x y] `(emit :render {:shape :point :x ~x :y ~y}))
(deftick
  (map (fn [e]
         (let [p (:pos e) spare 1]
           (point-row (:x p) (:y p))))
       (entities-where (fn [e] (= e.render :sprite)))))
(deftick
  (map (fn [e]
         (let [p (:pos e)] (point-row (:x p) (:y p))))
       (entities-where (fn [e] (= e.render :sprite)))))
(defpattern p [] (spawn (pose c[5 6]) {:render :sprite}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        assert!(sim.world.standing_rules[0].compiled[0].is_none());
        assert!(sim.world.standing_rules[1].compiled[0].is_some());
        sim.step().unwrap();
        assert_eq!(sim.render().len(), 2);
        assert!(sim.render().iter().all(|row| matches!(row.data,
            RenderData::Point { x: 5.0, y: 6.0, .. })));
    }

    #[test]
    fn compiled_value_or_preserves_keyword_field() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (emit :render {:shape :point :tag (value-or (:tag e) 5)}))
       (entities-where (fn [e] (= e.render :sprite)))))
(defpattern p [] (spawn (pose c[0 0]) {:render :sprite :tag :kept}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        assert!(sim.world.standing_rules[0].compiled[0].is_some());
        sim.step().unwrap();
        assert_eq!(sim.render()[0].sym("tag"), Some("kept"));
    }

    #[test]
    fn literal_and_computed_render_maps_match() {
        const LITERAL: &str = r#"
(deftick
  (render {:shape :point
           :x 1
           :y 2
           :theta 30
           :scale 3
           :alpha 0.5
           :hue 120
           :team :enemy
           :damage 4}))
(defpattern p [] (wait 1))
"#;
        const COMPUTED: &str = r#"
(deftick
  (let [row {:shape :point
             :x 1
             :y 2
             :theta 30
             :scale 3
             :alpha 0.5
             :hue 120
             :team :enemy
             :damage 4}]
    (render row)))
(defpattern p [] (wait 1))
"#;
        let mut literal = Sim::load(LITERAL, Some("p")).unwrap();
        let mut computed = Sim::load(COMPUTED, Some("p")).unwrap();
        literal.step().unwrap();
        computed.step().unwrap();
        let literal_rows = literal.render();
        let computed_rows = computed.render();
        assert_eq!(literal_rows.len(), 1);
        assert_eq!(computed_rows.len(), 1);
        assert_render_rows_eq(&literal_rows[0], &computed_rows[0]);
    }

    #[test]
    fn literal_render_map_alias_precedence() {
        const FACING: &str = r#"
(deftick (render {:shape :point :facing 45}))
(defpattern p [] (wait 1))
"#;
        let mut sim = Sim::load(FACING, Some("p")).unwrap();
        sim.step().unwrap();
        let RenderData::Point { theta, .. } = sim.render()[0].data else {
            panic!("render should emit one point row");
        };
        assert!((theta - 45.0).abs() < 1e-9, "theta: {theta}");

        const THETA_WINS: &str = r#"
(deftick (render {:shape :point :facing 45 :theta 90}))
(defpattern p [] (wait 1))
"#;
        let mut sim = Sim::load(THETA_WINS, Some("p")).unwrap();
        sim.step().unwrap();
        let RenderData::Point { theta, .. } = sim.render()[0].data else {
            panic!("render should emit one point row");
        };
        assert!((theta - 90.0).abs() < 1e-9, "theta: {theta}");
    }

    #[test]
    fn literal_render_map_extra_fields_keep_schema_checks() {
        const CARD: &str = r#"
(deftick
  (render {:shape :point
           :family :orb
           :damage 3}))
(defpattern p [] (wait 1))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let rows = sim.render();
        assert_eq!(rows[0].sym("family"), Some("orb"));
        assert_eq!(rows[0].num("damage"), Some(3.0));

        const BAD: &str = r#"
(deftick
  (seq
    (render {:shape :point :damage 3})
    (render {:shape :point :damage :big})))
(defpattern p [] (wait 1))
"#;
        let mut sim = Sim::load(BAD, Some("p")).unwrap();
        let Err(err) = sim.step() else {
            panic!("mismatched render field kind unexpectedly succeeded");
        };
        assert!(err.contains("render: field :damage is Sym here but Num elsewhere"), "{err}");
    }

    #[test]
    fn deftick_render_emits_polyline_row() {
        const CARD: &str = r#"
(deftick
  (render {:shape :polyline
           :points [c[0 0] c[1 0] {:x 1 :y 1}]
           :active 1}))
(defpattern p [] (wait 1))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let items = sim.render();
        let Some(row) = items.first() else {
            panic!("render should emit one polyline row");
        };
        let RenderData::Polyline { points, active } = &row.data else {
            panic!("render should emit one polyline row");
        };
        assert_eq!(points, &vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
        assert!(*active);
    }

    #[test]
    fn stock_dot_traced_entities_emit_point_rows_per_sample() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (pather 1 (linear c[1 0]))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..5 {
            sim.step().unwrap();
        }
        let points = sim.render()
            .into_iter()
            .filter(|r| matches!(r.data, RenderData::Point { .. }))
            .count();
        assert!(points >= 2, "expected one stock dot row per trace sample, got {points}");
    }

    #[test]
    fn rule_render_pose_emits_point_row() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (render {:shape :point
                  :x (:x (:pos e))
                  :y (:y (:pos e))
                  :scale e.scale
                  :alpha e.opacity
                  :hue e.hue
                  :theta e.facing}))
       (entities-where {:render :sprite})))
(defpattern p []
  (spawn (pose c[1 2])
         {:style {:family :orb :color :blue}
          :render :sprite
          :scale 2 :opacity 0.5 :hue 45 :facing 90}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let items = sim.render();
        let Some(row) = items.first() else {
            panic!("rule render should emit one point row");
        };
        let RenderData::Point { x, y, theta: th, scale, alpha, hue } = &row.data else {
            panic!("rule render should emit one point row");
        };
        assert!((*x - 1.0).abs() < 1e-9, "x: {x}");
        assert!((*y - 2.0).abs() < 1e-9, "y: {y}");
        assert!((*th - 90.0).abs() < 1e-9, "th: {th}");
        assert!((*scale - 2.0).abs() < 1e-9, "scale: {scale}");
        assert!((*alpha - 0.5).abs() < 1e-9, "alpha: {alpha}");
        assert!((*hue - 45.0).abs() < 1e-9, "hue: {hue}");
    }

    #[test]
    fn rule_render_pose_reads_dyn_entity_meta_field() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (render {:shape :point :scale e.scale}))
       (entities-where {:render :sprite})))
(defpattern p []
  (spawn (pose c[0 0]) {:render :sprite :scale (+ 1 t)}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        sim.step().unwrap();
        let items = sim.render();
        let Some(row) = items.first() else {
            panic!("rule render should emit one point row");
        };
        let RenderData::Point { scale, .. } = &row.data else {
            panic!("rule render should emit one point row");
        };
        assert!((*scale - (1.0 + 1.0 / DEFAULT_TICK_RATE)).abs() < 1e-9, "scale: {scale}");
    }

    #[test]
    fn rule_render_parametric_reads_entity_meta() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (render {:shape (curve-samples e {:resolution e.resolution :u-max e.u-max})
                  :active 1
                  :width e.width
                  :family e.family
                  :color e.color}))
       (entities-where {:render :beam})))
(defpattern p []
  (spawn ((pose c[-2 0]) (curve {:u-max 6}))
         {:style {:family :laser :color :red}
          :render :beam :warn 0 :active 2 :u-max 6 :resolution 0.1 :width 1}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert!(
            sim.render().iter().any(|r| {
                matches!(&r.data, RenderData::Polyline { active, .. } if *active)
            }),
            "rule render should emit a polyline row"
        );
    }

    #[test]
    fn rule_render_parametric_yields_multiple_polyline_rows() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (seq
           (render {:shape (curve-samples e {:resolution 0.5 :u-max 1}) :active 0})
           (render {:shape (curve-samples e {:resolution 0.5 :u-max 2}) :active 1})))
       (entities-where {:render :beam})))
(defpattern p []
  (spawn (curve {:u-max 3}) {:render :beam}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let polys: Vec<(bool, usize)> = sim.render()
            .iter()
            .filter_map(|r| match &r.data {
                RenderData::Polyline { points, active } => Some((*active, points.len())),
                _ => None,
            })
            .collect();
        assert_eq!(polys, vec![(false, 3), (true, 5)]);
    }

    #[test]
    fn touhou_beam_cull_is_library_policy() {
        const CARD: &str = r#"
(import "touhou")
(defpattern p []
  (par
    (laser ((pose c[0 0]) (curve {:u-max 1})) {:warn 0.1 :active 0.2})
    (laser ((pose c[0 1]) (curve {:u-max 1})) {:warn 0.1})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..80 {
            sim.step().unwrap();
        }
        let mut finite_alive = false;
        let mut default_alive = false;
        for i in 0..sim.world.entities.len() {
            if sim.world.col_get_at(i, "active") == Some(0.2) {
                finite_alive |= sim.world.entities.is_alive(i);
            } else if sim.world.col_get_at(i, "warn") == Some(0.1) {
                default_alive |= sim.world.entities.is_alive(i);
            }
        }
        assert!(!finite_alive, "finite active laser should be culled by touhou deftick");
        assert!(default_alive, "default active laser should survive");
    }

    #[test]
    fn curve_samples_rejects_non_curve_entity() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (render {:shape (curve-samples e {:resolution 0.1})}))
       (entities-where {:render :beam})))
(defpattern p [] (spawn (pose c[0 0]) {:render :beam}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        let Err(err) = sim.step() else {
            panic!("curve-samples on a non-curve unexpectedly succeeded");
        };
        assert!(err.contains("curve-samples entity is not a live curve"), "{err}");
    }

    #[test]
    fn curve_samples_rejects_unknown_option_key() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (render {:shape (curve-samples e {:bogus 1})}))
       (entities-where {:render :beam})))
(defpattern p [] (spawn (curve {:u-max 3}) {:render :beam}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        let Err(err) = sim.step() else {
            panic!("curve-samples unknown option unexpectedly succeeded");
        };
        assert!(err.contains("curve-samples: unknown option :bogus"), "{err}");
    }

    #[test]
    fn curve_samples_row_active_field_controls_polyline() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (render {:shape (curve-samples e {:resolution 0.5 :u-max 1})
                  :active 0}))
       (entities-where {:render :beam})))
(defpattern p [] (spawn (curve {:u-max 3}) {:render :beam}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let rows = sim.render();
        let Some(RenderData::Polyline { active, .. }) = rows.iter().map(|r| &r.data).next() else {
            panic!("expected one curve-samples polyline row");
        };
        assert!(!*active);
    }

    #[test]
    fn circle_collider_accepts_dynamic_radius() {
        const CARD: &str = r#"
(defcollider expanding-collider [e ctx]
  (circle-collider {:layer :expanding :r m"ctx.t"}))
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit))))
       (collisions :expanding :body)))
(defpattern p []
  (par
    (spawn (pose c[0 0])
           (circle-collider {:layer :body :r 0.05}))
    (spawn (pose c[1 0])
           expanding-collider)))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        assert!(
            sim.events_vec().iter().all(|e| &*e.name != "hit"),
            "radius is still too small before local projector time grows"
        );
        for _ in 0..120 {
            sim.step().unwrap();
        }
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "hit"),
            "time-dependent radius inside a projector is sampled at collision time"
        );
    }

    #[test]
    fn primitive_collider_outside_projector_cannot_read_entity_context() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit (:pos b)))))
       (collisions :damage :body)))
(defpattern p []
  (par
    (spawn (pose c[0 0])
           {:hitbox 0.3}
           (circle-collider {:layer :damage :r e.hitbox}))
    (spawn (pose c[0.35 0])
           (circle-collider {:layer :body :r 0.1}))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        let Err(err) = sim.step() else {
            assert!(false, "spawn-level primitive unexpectedly captured entity context");
            return;
        };
        assert!(err.contains("projector scope"), "{err}");
    }

    #[test]
    fn cond_controls_projector_output() {
        const CARD: &str = r#"
(defcollider appears-after [e ctx]
  (cond
    (> ctx.t 0.5) (circle-collider {:layer :appears :r 0.1})
    :else (circle-collider {:layer :cold :r 0.01})))
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit))))
       (collisions :appears :body)))
(defpattern p []
  (par
    (spawn (pose c[0 0])
           (circle-collider {:layer :body :r 0.05}))
    (spawn (pose c[0 0])
           appears-after)))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        assert!(
            sim.events_vec().iter().all(|e| &*e.name != "hit"),
            "empty dynamic collider list should be inert"
        );
        for _ in 0..60 {
            sim.step().unwrap();
        }
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "hit"),
            "whole dynamic collider list should decode after per-tick realization"
        );
    }

    #[test]
    fn spawn_level_cond_cannot_capture_projector_context() {
        const CARD: &str = r#"
(deftick
  (map (fn [[a b]]
         (if (< (if (nothing? (:hit a)) 0 (:hit a)) 1)
           (seq (set-col a :hit 1) (event :hit))))
       (collisions :appears :body)))
(defpattern p []
  (par
    (spawn (pose c[0 0])
           (circle-collider {:layer :body :r 0.05}))
    (spawn (pose c[0 0])
           (cond
             (> ctx.t 0.5) (circle-collider {:layer :appears :r 0.1})
             :else (circle-collider {:layer :cold :r 0.01})))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        let Err(err) = sim.step() else {
            assert!(false, "spawn-level cond unexpectedly captured projector context");
            return;
        };
        assert!(err.contains("projector scope"), "{err}");
    }

    /// The duel-card bug: aim inside an expression-level frame must aim
    /// FROM that frame's position (the frame is ambient for its body),
    /// not from the world origin. Player just below the source → entities
    /// head down at the player, not up.
    /// Lifecycle trees: handles + per-entity forked timelines express
    /// multi-stage lifecycles with no queries — (for [b handles] …)
    /// iterates an array in the lead binding.
    #[test]
    fn lifecycle_tree_via_handles() {
        const CARD: &str = r#"
(defpattern p []
  (let [ring (spawn (circle 4 (linear p[1.5 0]))
                    {:style {:family :circle}})]
    (for [b ring, i (iota 4)]
      (fork
        (seq
          (wait 0.5)
          (seq
            ((pose (pos b))
              (spawn (nth [(circle 6 (linear p[2 0]))
                           (fan 3 20 (linear p[2 0]))]
                          i)
                     {:style {:family (nth [:gem :star] i)}}))
            (cull b)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..90 {
            sim.step().unwrap();
        }
        let count = |f: &str| live_family_count(&sim, f);
        assert_eq!(count("circle"), 0, "stage-1 entities consumed");
        assert_eq!(count("gem"), 12, "even indices: two 6-rings");
        assert_eq!(count("star"), 6, "odd indices: two 3-fans");
    }

    /// Invulnerability windows: (invuln b dur) writes iframe-until, which
    /// BOTH resolve paths honor — shots are absorbed (die, no hp write)
    /// while a boss is invulnerable, and hp flows again after expiry.
    #[test]
    fn invuln_window_absorbs_damage() {
        const CARD: &str = r#"
(import "touhou")
(defpattern p []
  (let [boss (enemy (pose c[0 3]) {:hp 10})]
    (seq
      (invuln (nth boss 0) 1)
      (fork
        (for [i inf :every (ticks 30)]
          ((pose c[0 0])
            (shot (vel c[0 6]) {:damage 1})))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        // boss at y=3, shots at 6/s reach it in ~60 ticks; invuln covers
        // the first second (120 ticks) — early shots are absorbed
        for _ in 0..115 {
            sim.step().unwrap();
        }
        let hp = |sim: &Sim| {
            sim.world
                .entities
                .iter()
                .enumerate()
                .find(|(i, _)| sim.world.sym_field_matches_at(*i, "team", "enemy"))
                .and_then(|(i, _)| sim.world.col_get_at(i, "hp"))
                .unwrap()
        };
        assert_eq!(hp(&sim), 10.0, "shots absorbed during the window");
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "absorbed"),
            "absorption is observable"
        );
        for _ in 0..240 {
            sim.step().unwrap();
        }
        assert!(hp(&sim) < 10.0, "damage flows after the window expires");
    }

    /// Dyn application with a numeric first argument samples a named closed
    /// dyn without expressing an entity.
    #[test]
    fn dyn_application_samples_named_closed_dyn() {
        const CARD: &str = r#"
(def d (cart (* 2 t) (+ 1 t)))
(defpattern p []
  (spawn (d 3.5) {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let (x, y) = sim.world.entities.sampled_pos(0, sim.world.tick - 1).unwrap_or((f64::NAN, f64::NAN));
        assert!((x - 7.0).abs() < 1e-6 && (y - 4.5).abs() < 1e-6, "sampled pose: ({}, {})", x, y);
    }

    /// Plain functions are accepted in dyn pose slots and evaluated as f(t).
    #[test]
    fn fn_in_pose_slot_moves_entity() {
        const CARD: &str = r#"
(def wobble (fn [t] (cart (* 20 (cos (* 3 t))) 0)))
(defpattern p []
  (spawn wobble {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let state = MotionState::new();
        let sig = SigEnv::default();
        let p = dyn_figure_pose(dyn_figure(&sim, 0), 0.5, &state, &sig).unwrap();
        let want = 20.0 * (1.5f64).to_radians().cos();
        assert!((p.x - want).abs() < 1e-6 && p.y.abs() < 1e-6, "fn-backed dyn pose: {:?}", p);
    }

    /// Curves are values: (curve t u) evaluates a u-parameterized dyn without
    /// expressing an entity — pose plus tangent heading.
    #[test]
    fn dyn_application_samples_curves() {
        const CARD: &str = r#"
(defpattern p []
  (let [shape (polar m"2 * u" 0)]
    (spawn ((pose (shape 0 1)) (pose c[0 0]))
           {:style {:family :gem}})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let (x, y) = sim.world.entities.sampled_pos(0, sim.world.tick - 1).unwrap_or((f64::NAN, f64::NAN));
        assert!((x - 2.0).abs() < 1e-6 && y.abs() < 1e-6, "point at u=1 on a straight radial curve: ({}, {})", x, y);
    }

    /// A scalar evolve sampled by application inside a fn-backed dyn: the
    /// fold replays from epoch start, one step per tick.
    #[test]
    fn evolve_scalar_state_integrates() {
        const CARD: &str = r#"
(def rise (evolve 0 (fn [s c] (+ s (* 60 (:dt c))))))
(defpattern p []
  (spawn (fn [t] (cart (rise t) 0)) {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let state = MotionState::new();
        // top-level defs resolve via SigEnv::defs, so use the sim's sig
        let sig = sim.ctx.sig.clone();
        let p = dyn_figure_pose(dyn_figure(&sim, 0), 0.5, &state, &sig).unwrap();
        // 120 Hz: 60 steps of +60·dt = +0.5 each.
        assert!((p.x - 30.0).abs() < 1e-6 && p.y.abs() < 1e-6, "evolved scalar: {:?}", p);
    }

    /// A pose-state evolve coerces into a pose slot directly.
    #[test]
    fn evolve_pose_state_in_pose_slot() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (evolve (cart 0 0) (fn [s c] (cart (+ (:x s) 1) (:y s))))
         {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let state = MotionState::new();
        let sig = SigEnv::default();
        let p = dyn_figure_pose(dyn_figure(&sim, 0), 0.5, &state, &sig).unwrap();
        assert!((p.x - 60.0).abs() < 1e-6 && p.y.abs() < 1e-6, "evolved pose: {:?}", p);
    }

    /// Evolve state is any value: a map-carrying evolve sampled by
    /// application, with the ctx tick counter driving the step.
    #[test]
    fn evolve_map_state_samples_any_value() {
        const CARD: &str = r#"
(def m (evolve {:x 0} (fn [s c] {:x (+ (:x s) (:tick c))})))
(defpattern p []
  (spawn (fn [t] (cart (:x (m t)) 0)) {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let state = MotionState::new();
        let sig = sim.ctx.sig.clone();
        let p = dyn_figure_pose(dyn_figure(&sim, 0), 0.5, &state, &sig).unwrap();
        // ticks 0..59 summed = 1770.
        assert!((p.x - 1770.0).abs() < 1e-6, "evolved map state: {:?}", p);
    }

    fn find_evolve(node: &Rc<DynNode>) -> Rc<EvolveDyn> {
        match &**node {
            DynNode::Evolve(ev) => ev.clone(),
            DynNode::Translate { child, .. } | DynNode::Clamp { child, .. } => find_evolve(child),
            DynNode::Frame(a, b) => {
                if let DynNode::Evolve(_) = &**a {
                    find_evolve(a)
                } else {
                    find_evolve(b)
                }
            }
            other => panic!("expected evolve node, got {other:?}"),
        }
    }

    #[test]
    fn evolve_pose_slot_has_dense_val_state() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (evolve (cart 0 0) (fn [s c] (cart (+ (:x s) 1) (:y s))))
         {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let schema = sim.world.entities.motion_schema(0).unwrap();
        assert_eq!(schema.val_keys.len(), 1);
        assert!(schema.n2_keys.is_empty());
        assert!(schema.dyn_keys.is_empty());
        let key = schema.val_keys[0];
        let cell = sim.world.entities.state_val(0, key).unwrap();
        assert_eq!(cell.tick, 0);
        assert!(matches!(cell.state, Val::Pose(p) if p.x.abs() < 1e-9 && p.y.abs() < 1e-9));
    }

    #[test]
    fn evolve_on_clock_matches_replay_and_off_clock_still_replays() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (evolve (cart 0 0) (fn [s c] (cart (+ (:x s) 1) (:y s))))
         {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..12 {
            sim.step().unwrap();
        }
        let readers = sim.motion_readers(0);
        let state = MotionState::new();
        let sig = sim.ctx.sig.clone();
        let ev = find_evolve(dyn_figure(&sim, 0).pose_dyn());
        let settled_tau = (sim.world.tick - 1) as f64 / DEFAULT_TICK_RATE;
        let memo = dyn_figure_pose_in(
            dyn_figure(&sim, 0),
            settled_tau,
            MotionEvalCtx::new(&state, &sig, &readers),
        )
        .unwrap();
        let replay = match evolve_value(&ev, settled_tau, &sig, DEFAULT_TICK_RATE).unwrap() {
            Val::Pose(p) => p,
            other => panic!("expected replay pose, got {other:?}"),
        };
        assert!((memo.x - replay.x).abs() < 1e-9 && (memo.y - replay.y).abs() < 1e-9);

        let off_clock = dyn_figure_pose_in(
            dyn_figure(&sim, 0),
            3.5 / DEFAULT_TICK_RATE,
            MotionEvalCtx::new(&state, &sig, &readers),
        ).unwrap();
        assert!((off_clock.x - 3.0).abs() < 1e-9, "off-clock replay pose: {off_clock:?}");
    }

    #[test]
    fn evolve_liveness_is_rooted_at_the_step_params() {
        let read = |s: &str| crate::edn::read_one(s).unwrap();
        // fold-internal keyword access (state, step ctx, chains) stays closed
        assert!(!evolve_is_live(&read("0"), &read("(fn [s c] (+ s (* 60 (:dt c))))")));
        assert!(!evolve_is_live(&read("(cart 0 0)"), &read("(fn [s c] (:x (:vel s)))")));
        assert!(!evolve_is_live(&read("(:x {:x 1})"), &read("(fn [s c] s)")));
        // capture-rooted access, channels, and world reads are live
        assert!(evolve_is_live(&read("0"), &read("(fn [s c] (+ s (:hp e)))")));
        assert!(evolve_is_live(&read("(:pos e)"), &read("(fn [s c] s)")));
        assert!(evolve_is_live(&read("0"), &read("(fn [s c] (+ s $dx))")));
        assert!(evolve_is_live(&read("0"), &read("(fn [s c] (nearest-entity pos))")));
    }

    #[test]
    fn live_evolve_reads_current_channel_and_rejects_off_clock_sampling() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (evolve (cart 0 0) (fn [s c] (cart (+ (:x s) $dx) 0)))
         {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for value in [1.0, 2.0, 3.0] {
            sim.step_with(&Inputs { vals: vec![("dx".into(), Val::Num(value))] }).unwrap();
        }
        let schema = sim.world.entities.motion_schema(0).unwrap();
        let cell = sim.world.entities.state_val(0, schema.val_keys[0]).unwrap();
        assert!(matches!(cell.state, Val::Pose(p) if (p.x - 5.0).abs() < 1e-9), "cell: {:?}", cell);
        let err = dyn_figure_pose_in(
            dyn_figure(&sim, 0),
            20.0 / DEFAULT_TICK_RATE,
            MotionEvalCtx::new(&MotionState::new(), &sim.ctx.sig, &sim.motion_readers(0)),
        ).unwrap_err();
        assert_eq!(err, "live evolve sampled off its clock");
    }

    #[test]
    fn evolve_init_is_deferred_and_captures_lexical_env() {
        const GOOD: &str = r#"
(defpattern p []
  (let [origin (cart 4 5)]
    (spawn (evolve origin (fn [s c] s)) {:style {:family :gem}})))
"#;
        let mut sim = Sim::load(GOOD, Some("p")).unwrap();
        sim.step().unwrap();
        let schema = sim.world.entities.motion_schema(0).unwrap();
        assert!(matches!(sim.world.entities.state_val(0, schema.val_keys[0]).unwrap().state,
            Val::Pose(p) if (p.x - 4.0).abs() < 1e-9 && (p.y - 5.0).abs() < 1e-9));

        const BAD: &str = r#"
(def bad (evolve (unknown-init) (fn [s c] s)))
(defpattern p [] (spawn bad {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(BAD, Some("p")).unwrap();
        assert!(sim.step().unwrap_err().contains("unknown-init"));
    }

    #[test]
    fn remat_live_evolve_restarts_with_post_remat_entity_pose() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (vel (cart 120 0)) {:style {:family :gem}})]
    (seq (wait (ticks 3))
         (let [e (first bs)]
           (remat e (evolve (:pos e) (fn [s c] s)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..6 {
            sim.step().unwrap();
        }
        let schema = sim.world.entities.motion_schema(0).unwrap();
        let cell = sim.world.entities.state_val(0, schema.val_keys[0]).unwrap();
        assert!(matches!(cell.state, Val::Pose(p)
            if (p.x - 4.0).abs() < 1e-9 && p.y.abs() < 1e-9),
            "cell: {:?}", cell);
    }

    #[test]
    fn evolve_dense_state_advances_linearly() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (evolve (cart 0 0) (fn [s c] (cart (+ (:x s) 1) (:y s))))
         {:style {:family :gem}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..10 {
            sim.step().unwrap();
        }
        let schema = sim.world.entities.motion_schema(0).unwrap();
        let cell = sim.world.entities.state_val(0, schema.val_keys[0]).unwrap();
        assert_eq!(cell.tick, sim.world.tick - 1);
        assert!(matches!(cell.state, Val::Pose(p) if (p.x - cell.tick as f64).abs() < 1e-9));
    }

    #[test]
    fn evolve_snapshot_restore_is_deterministic() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (evolve (cart 0 0) (fn [s c] (cart (+ (:x s) 1) (:y s))))
         {:style {:family :gem}}))
"#;
        let mut a = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..4 {
            a.step().unwrap();
        }
        let mut b = a.clone();
        for _ in 0..6 {
            a.step().unwrap();
            b.step().unwrap();
        }
        let a_schema = a.world.entities.motion_schema(0).unwrap();
        let b_schema = b.world.entities.motion_schema(0).unwrap();
        let ac = a.world.entities.state_val(0, a_schema.val_keys[0]).unwrap();
        let bc = b.world.entities.state_val(0, b_schema.val_keys[0]).unwrap();
        assert_eq!(ac.tick, bc.tick);
        assert!(matches!((ac.state, bc.state), (Val::Pose(ap), Val::Pose(bp)) if (ap.x - bp.x).abs() < 1e-12 && (ap.y - bp.y).abs() < 1e-12));
    }

    #[test]
    fn remat_motion_clears_evolve_state_epoch() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (evolve (cart 0 0) (fn [s c] (cart (+ (:x s) 1) 0)))
                  {:style {:family :gem}})]
    (seq (wait (ticks 3))
         (remat (first bs) (evolve (cart 0 0) (fn [s c] (cart (+ (:x s) 10) 0)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..6 {
            sim.step().unwrap();
        }
        let schema = sim.world.entities.motion_schema(0).unwrap();
        let cell = sim.world.entities.state_val(0, schema.val_keys[0]).unwrap();
        assert!(cell.tick <= 2, "remat should restart evolve epoch, got {cell:?}");
        assert!(matches!(cell.state, Val::Pose(p) if p.x <= 20.0));
    }

    /// Slow lasers: the telegraph shows the whole path immediately, but
    /// the hitbox sweeps out from the source over the :fill window.
    #[test]
    fn slow_laser_fills() {
        const CARD: &str = r#"
(import "touhou")
(defpattern rig []
  (let [p (spawn (live $player)
                 (circle-collider {:layer :player-hurt :r 0.06})
                 {:team :player-body
                  :graze 0 :hits 0})]
    (let [body (nth p 0)]
      (bind-channel! $graze (:graze body))
      (bind-channel! $hits (:hits body)))))
(defpattern beam []
  (par (rig)
       (laser ((pose c[-2 0]) (curve {:u-max 6}))
              {:warn 0.5 :active 6 :u-max 6 :fill (fill-linear 0.5 2)
               :style {:family :laser :color :red}})))
"#;
        let mut sim = Sim::load(CARD, Some("beam")).unwrap();
        // player parked on the beam line at u = 2 (x = 0); 120 ticks/s
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        // warn ends at 0.5s (tick 60); the front reaches u=2 at
        // tau = 0.5 + (2/6)*2 ≈ 1.17s (tick ~140, less the capsule radii)
        for _ in 0..100 {
            sim.step_with(&inputs).unwrap(); // t ≈ 0.83s: front at u ≈ 1.0
        }
        assert_eq!(sim.channel_u64("hits"), 0, "front hasn't reached the player");
        for _ in 0..60 {
            sim.step_with(&inputs).unwrap(); // t ≈ 1.33s: front at u = 2.5
        }
        assert_eq!(sim.channel_u64("hits"), 1, "the sweeping front arrived");
        // full path is still telegraphed while filling: dim + bright polylines
        let mut sim2 = Sim::load(CARD, Some("beam")).unwrap();
        for _ in 0..90 {
            sim2.step_with(&inputs).unwrap(); // t = 0.75s: mid-fill
        }
        let polys: Vec<bool> = sim2
            .render()
            .iter()
            .filter_map(|r| match &r.data {
                RenderData::Polyline { active, .. } => Some(*active),
                _ => None,
            })
            .collect();
        assert_eq!(polys, vec![false, true], "dim full path + bright hot prefix");
    }

    #[test]
    fn move_can_target_an_entity() {
        const CARD: &str = r#"
(import "touhou")
(defpattern p []
  (let [enemy (enemy (pose c[0 0]) {:style {:family :lstar}})]
    (move-to (nth enemy 0) 1.0 eoutsine c[2 0])))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..60 {
            sim.step().unwrap();
        }
        let tau = sim.world.entity_tau(0, sim.world.tick);
        let state = MotionState::new();
        let readers = sim.motion_readers(0);
        let p = dyn_figure_pose_in(dyn_figure(&sim, 0), tau, MotionEvalCtx::new(&state, &sim.ctx.sig, &readers)).unwrap();
        let mid = 2.0 * (0.5f64 * std::f64::consts::FRAC_PI_2).sin();
        assert!((p.x - mid).abs() < 0.03 && p.y.abs() < 1e-9, "mid-move pose: {:?}", p);

        for _ in 0..70 {
            sim.step().unwrap();
        }
        let tau = sim.world.entity_tau(0, sim.world.tick);
        let state = MotionState::new();
        let readers = sim.motion_readers(0);
        let p = dyn_figure_pose_in(dyn_figure(&sim, 0), tau, MotionEvalCtx::new(&state, &sim.ctx.sig, &readers)).unwrap();
        assert!((p.x - 2.0).abs() < 1e-9 && p.y.abs() < 1e-9, "final pose: {:?}", p);
    }

    #[test]
    fn aim_sees_expression_frame_ambient() {
        const CARD: &str = r#"
(defpattern nested []
  (spawn (in-frame (pose c[0 3]) ((aim $player) (linear p[2 0])))))
(defpattern flat []
  (spawn (in-frame (pose c[0 3]) (aim $player) (linear p[2 0]))))
"#;
        for pat in ["nested", "flat"] {
            let mut sim = Sim::load(CARD, Some(pat)).unwrap();
            // player below the source: pre-fix, aim measured from (0,0)
            // and fired UP toward (0,1); the bullet must head DOWN
            let inputs = Inputs::classic((0.0, 1.0), (0.0, 1.0));
            for _ in 0..60 {
                sim.step_with(&inputs).unwrap();
            }
            let sig = SigEnv::default();
            let state = MotionState::new();
            let p = dyn_figure_pose(dyn_figure(&sim, 0), 0.5, &state, &sig).unwrap();
            assert!(
                p.x.abs() < 1e-9 && (p.y - 2.0).abs() < 1e-9,
                "{}: fired from (0,3) toward the player below: {:?}",
                pat,
                p
            );
        }
    }

    /// (in-frame f1 f2 body) folds the frame monoid: same pose as nesting.
    #[test]
    fn in_frame_variadic() {
        const CARD: &str = r#"
(defpattern flat []
  (spawn (in-frame (pose c[0 1]) (rot 90) (linear c[1 0]))))
(defpattern nested []
  (spawn (in-frame (pose c[0 1]) (in-frame (rot 90) (linear c[1 0])))))
"#;
        let mut a = Sim::load(CARD, Some("flat")).unwrap();
        let mut b = Sim::load(CARD, Some("nested")).unwrap();
        for _ in 0..60 {
            a.step().unwrap();
            b.step().unwrap();
        }
        let sig = SigEnv::default();
        let state = MotionState::new();
        let pa = dyn_figure_pose(dyn_figure(&a, 0), 0.5, &state, &sig).unwrap();
        let pb = dyn_figure_pose(dyn_figure(&b, 0), 0.5, &state, &sig).unwrap();
        assert!((pa.x - pb.x).abs() < 1e-12 && (pa.y - pb.y).abs() < 1e-12);
        // rot 90 turns +x motion into +y, from anchor (0,1): at t=0.5 → (0, 1.5)
        assert!(pa.x.abs() < 1e-9 && (pa.y - 1.5).abs() < 1e-9, "got {:?}", pa);
    }

    /// The event log is bounded: old events prune once past the size
    /// threshold, keeping snapshot cost O(world), not O(elapsed time).
    #[test]
    fn event_log_bounded() {
        const CARD: &str = r#"
(defpattern chatty [] (dotimes [i inf :every (ticks 1)] (event :ping)))
"#;
        let mut sim = Sim::load(CARD, Some("chatty")).unwrap();
        for _ in 0..6000 {
            sim.step().unwrap();
        }
        let events = sim.events_vec();
        assert!(events.len() < 4200, "pruned: {}", events.len());
        let newest = events.last().unwrap().tick;
        assert!(newest >= 5990, "recent events kept");
    }

    /// (import "path") splices recursively, include-once: importing two
    /// files that both import a common base yields one copy of the base.
    #[test]
    fn imports_expand_once() {
        let dir = std::env::temp_dir().join("maku-import-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("base.maku"), "(def shared 7)\n").unwrap();
        std::fs::write(
            dir.join("a.maku"),
            "(import \"base.maku\")\n(defpattern a [] (spawn (pose c[shared 0])))\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("main.maku"),
            "(import \"a.maku\")\n(import \"base.maku\") ; already included\n\
             (defpattern m [] (a))\n",
        )
        .unwrap();
        let src = crate::edn::expand_card(&dir.join("main.maku")).unwrap();
        assert_eq!(src.matches("(def shared 7)").count(), 1, "include-once");
        let mut sim = Sim::load(&src, Some("m")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.entities.len(), 1, "imported defs resolve through layers");
    }

    /// (until pred body): the tick the predicate holds, the body's whole
    /// task subtree — including forks — dies. §8 phase-end cancellation.
    #[test]
    fn until_cancels_subtree() {
        const CARD: &str = r#"
(defpattern u []
  (defcell stop 0)
  (par
    (until (= stop 1)
      (par (fork (dotimes [i inf :every (ticks 5)]
                   (spawn (linear c[0.01 0]))))
           (dotimes [j inf :every (ticks 5)]
             (spawn (linear c[0 0.01])))))
    (seq (wait (ticks 52)) (set! stop 1))))
"#;
        let mut sim = Sim::load(CARD, Some("u")).unwrap();
        for _ in 0..60 {
            sim.step().unwrap();
        }
        let at_cancel = sim.world.entities.len();
        assert!(at_cancel >= 20, "both spawners ran: {}", at_cancel);
        for _ in 0..200 {
            sim.step().unwrap();
        }
        assert_eq!(
            sim.world.entities.len(),
            at_cancel,
            "cancelled subtree (loop AND its fork) spawns nothing more"
        );
    }

    /// (clamp lo hi dyn) clamps the INTEGRATOR state: pushing a wall banks
    /// no phantom distance — reversing moves away immediately.
    #[test]
    fn clamp_slides_not_banks() {
        const CARD: &str = r#"
(defpattern c []
  (let [h (spawn (clamp c[-2 -2] c[2 2]
                   (in-frame c[0 -1] (vel c[(* 4 (live $move-x)) 0])))
                 (circle-collider {:layer :player-hurt :r 0.05})
                 {:team :player-body
                  :pilot 1})]
    (bind-channel! $player (:pos (nth h 0)))))
"#;
        let mut sim = Sim::load(CARD, Some("c")).unwrap();
        let mut inputs = Inputs::default();
        // push left 480 ticks (would travel 16 units unclamped)
        inputs.set_num("move-x", -1.0);
        for _ in 0..480 {
            sim.step_with(&inputs).unwrap();
        }
        let x_wall = match sim.channel_val("player") {
            Some(Val::Pose(p)) => p.x,
            v => panic!("bad player channel: {:?}", v),
        };
        assert!((x_wall + 2.0).abs() < 0.05, "parked at the wall: {}", x_wall);
        // reverse for half a second: must move ~2 units immediately
        inputs.set_num("move-x", 1.0);
        for _ in 0..60 {
            sim.step_with(&inputs).unwrap();
        }
        let x_back = match sim.channel_val("player") {
            Some(Val::Pose(p)) => p.x,
            _ => unreachable!(),
        };
        assert!(x_back > -0.2, "no banked phantom distance: {}", x_back);
    }

    /// Macros: unevaluated arguments, backtick templates, splicing; the
    /// expansion evaluates in the caller's scope.
    #[test]
    fn macros_expand() {
        const CARD: &str = r#"
(defmacro where [expr] `(fn [b] ~expr))
(defmacro ring-every [n dt body]
  `(for [vol inf :every ~dt] (spawn (circle ~n ~body))))
(defpattern p []
  (par
    (ring-every 6 0.5 (linear p[2 0]))
    (fork (for [i inf :every (ticks 5)]
      (manip (where (> b.t 0.8)) (fn [b] (cull b)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..200 {
            sim.step().unwrap();
        }
        // rings keep spawning; the predicate control ages them out
        let n = sim.world.entities.len();
        assert!(n >= 6 && n <= 18, "steady state through macro sugar: {}", n);
    }

    /// Per-element numeric fields (arrays bind like style axes) and
    /// deferred forks (timed work scheduled from inside a callback).
    #[test]
    fn cols_per_element_and_deferred_fork() {
        const CARD: &str = r#"
(defpattern p []
  (seq
    (spawn (circle 4 (linear c[0.5 0]))
           {:style {:family :seed} :ci (iota 4)})
    (wait (ticks 2))
    (manip (fn [b] (* (= b.family :seed) (> b.ci 2.5)))
      (fn [b]
        (seq
          (fork (seq (wait (ticks 10))
                     (spawn (circle 6 (linear c[1 0]))
                            {:style {:family :burst}})))
          (cull b))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..5 {
            sim.step().unwrap();
        }
        // only the seed with ci=3 matched the query and died
        assert_eq!(
            live_family_count(&sim, "seed"),
            3,
            "per-element column selected exactly one seed"
        );
        assert_eq!(live_family_count(&sim, "burst"), 0);
        for _ in 0..15 {
            sim.step().unwrap();
        }
        // the deferred fork's timed spawn landed after its wait
        assert_eq!(
            live_family_count(&sim, "burst"),
            6,
            "callback-forked timed work ran as an adopted task"
        );
    }

    /// Accessor sugar: dotted symbols are keyword chains (reader-level);
    /// they read handles (live entity view), maps, and vectors; m-strings
    /// add postfix indexing with array gather.
    #[test]
    fn accessor_sugar() {
        const CARD: &str = r#"
(defpattern p []
  (seq
    (defcell probe 0)
    (export probe)
    (spawn (pose c[3 4]) {:style {:family :circle}})
    (manip (fn [b] (* (= b.family :circle) (> b.pos.y 1)))
      (fn [b] (set! probe b.pos.y)))))
(defpattern gather []
  (spawn ((rot m"(30 * iota(12)).[iota(3)]") (linear c[1 0]))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..3 {
            sim.step().unwrap();
        }
        assert!(
            matches!(sim.channel_val("probe"), Some(Val::Num(n)) if n == 4.0),
            "handle field through predicate query and callback: {:?}",
            sim.channel_val("probe")
        );
        let mut sim = Sim::load(CARD, Some("gather")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.entities.len(), 3, "m-string postfix array gather");
    }

    /// Nested meta arrays resolve structurally: depth = axis along the
    /// element's path, cycling per level, scalars broadcasting down.
    #[test]
    fn nested_meta_structural() {
        const CARD: &str = r#"
(defpattern p []
  (spawn ((rot m"30 * iota(10)")
           ((rot m"4 * iota(3)")
             ((pose c[1 0]) (linear p[2 0]))))
         {:style {:family :arrow
                  :color [[:red :blue] :green :purple]}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let col = |g: usize, i: usize| style(&sim, g * 3 + i).color.clone();
        assert_eq!(
            (col(0, 0), col(0, 1), col(0, 2)),
            ("red".into(), "blue".into(), "red".into()),
            "nested element cycles the inner axis"
        );
        assert_eq!(col(1, 0), "green", "scalar element broadcasts its group");
        assert_eq!(col(2, 2), "purple");
        assert_eq!(col(3, 1), "blue", "outer level cycles over the groups");
    }

    /// Tutorial cards are doctests: every example pattern in every
    /// cards/tutorials/*.maku must load and run (the docs can't rot).
    #[test]
    #[ignore = "long tutorial corpus test; run with cargo test --lib -- --ignored --test-threads=1"]
    fn tutorial_cards_run() {
        let dir = std::path::Path::new("../../cards/tutorials");
        let mut swept = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("maku") {
                continue;
            }
            let src = crate::edn::expand_card(&path).unwrap();
            let card = load_card(&read_all(&src).unwrap()).unwrap();
            for name in &card.order {
                let mut sim = Sim::load(&src, Some(name))
                    .unwrap_or_else(|e| panic!("{:?} [{}]: {}", path, name, e));
                for k in 0..240 {
                    sim.step().unwrap_or_else(|e| {
                        panic!("{:?} [{}] tick {}: {}", path, name, k, e)
                    });
                }
                assert!(
                    !sim.world.entities.is_empty() || sim.world.cursor > 0,
                    "{:?} [{}]: example did nothing visible",
                    path,
                    name
                );
                swept += 1;
            }
        }
        assert!(swept >= 9, "tutorial patterns swept: {}", swept);
    }

    /// The §10 embedding adapters: pattern instances get ISOLATED cells by
    /// default (two embeddings of the same pattern don't share defcell
    /// state); (inline …) shares the caller's scope; defns called from a
    /// pattern see its cells dynamically (spell-2's guide-rig idiom).
    #[test]
    fn embedding_adapters() {
        const CARD: &str = r#"
(defn helper-reads [] (spawn (circle (live n) (linear c[1 0]))))
(defpattern counter [start 1]
  (seq
    (defcell n start)
    (set! n (+ (live n) 1))
    (helper-reads)))                      ; defn sees THIS instance's n
(defpattern outer []
  (seq
    (defcell n 100)
    (export n)
    (par (counter 1) (counter 5))         ; isolated: 2 and 6, not shared
    (wait (ticks 2))
    (inline (bump))))                     ; inline: mutates OUR n
(defpattern bump []
  (set! n 200))
"#;
        let mut sim = Sim::load(CARD, Some("outer")).unwrap();
        for _ in 0..5 {
            sim.step().unwrap();
        }
        // two counter instances spawned rings of 2 and 6 — isolated cells
        // (shared cells would give 2 and 3, or collide with outer's 100)
        let mut sizes: Vec<usize> = Vec::new();
        let counts = sim.world.entities.len();
        assert_eq!(counts, 8, "2 + 6 entities: {}", counts);
        sizes.push(counts);
        // inline (bump) wrote through to OUTER's exported cell
        assert!(
            matches!(sim.channel_val("n"), Some(Val::Num(v)) if v == 200.0),
            "inline shares the caller's cells: {:?}",
            sim.channel_val("n")
        );
    }

    /// Handle-scoped derived channels can publish entity fields for gates;
    /// (export cell) publishes a pattern cell read-only.
    #[test]
    fn bind_channel_and_export() {
        const CARD: &str = r#"
(import "touhou")
(defchannel $target-hp 0)
(defpattern e []
  (seq
    (defcell phase 1)
    (export phase)
    (let [target (enemy (pose c[0 2]) {:hp 2 :hitbox 0.3})]
      (bind-channel! $target-hp (value-or (:hp (nth target 0)) 0)))
    (shot (in-frame (pose c[0 0]) (vel c[0 4])) {:damage 1})
    (wait-for (<= $target-hp 1))
    (set! phase 2)
    (shot (in-frame (pose c[0 0]) (vel c[0 4])) {:damage 1})))
"#;
        let mut sim = Sim::load(CARD, Some("e")).unwrap();
        for _ in 0..40 {
            sim.step().unwrap();
        }
        assert!(matches!(sim.channel_val("target-hp"), Some(Val::Num(n)) if n == 2.0));
        assert!(matches!(sim.channel_val("phase"), Some(Val::Num(n)) if n == 1.0));
        for _ in 0..40 {
            sim.step().unwrap(); // first shot lands ~tick 47; second ~95
        }
        assert!(matches!(sim.channel_val("target-hp"), Some(Val::Num(n)) if n == 1.0));
        assert!(
            matches!(sim.channel_val("phase"), Some(Val::Num(n)) if n == 2.0),
            "exported cell tracks the pattern's set!"
        );
        for _ in 0..220 {
            sim.step().unwrap(); // second shot kills; entity culled
        }
        assert!(
            matches!(sim.channel_val("target-hp"), Some(Val::Num(n)) if n == 0.0),
            "dead entity reads 0, not stale"
        );
    }

    /// Two pilots: distinct input channels move distinct rigs, channels
    /// derive per pilot-value, and iframes are per-entity — both pilots
    /// can be hit in the same window.
    #[test]
    fn two_players() {
        let rig = std::fs::read_to_string("../../cards/coop.maku").unwrap();
        let mut sim = Sim::load(&rig, Some("coop")).unwrap();
        let mut inputs = Inputs::default();
        // p1 pushes right, p2 pushes left — they cross
        inputs.set_num("p1-move-x", 1.0);
        inputs.set_num("p1-move-y", 0.0);
        inputs.set_num("p2-move-x", -1.0);
        inputs.set_num("p2-move-y", 0.0);
        for _ in 0..120 {
            sim.step_with(&inputs).unwrap();
        }
        let p1 = match sim.channel_val("player-1") {
            Some(Val::Pose(p)) => p.x,
            v => panic!("no $player-1: {:?}", v),
        };
        let p2 = match sim.channel_val("player-2") {
            Some(Val::Pose(p)) => p.x,
            v => panic!("no $player-2: {:?}", v),
        };
        assert!(p1 > -1.5 && p2 < 1.5, "rigs moved on their own channels: {} {}", p1, p2);
        assert!(matches!(sim.channel_val("lives-1"), Some(Val::Num(n)) if n == 3.0));
        assert!(sim.channel_val("nearest-pilot").is_some());

        // per-entity iframes: park both pilots in the aimed spray column —
        // over time BOTH lose lives (a global iframe would shield one)
        let mut inputs = Inputs::default();
        inputs.set_num("p1-move-x", 0.35); // drift toward center
        inputs.set_num("p1-move-y", 0.0);
        inputs.set_num("p2-move-x", -0.35);
        inputs.set_num("p2-move-y", 0.0);
        let mut sim = Sim::load(&rig, Some("coop")).unwrap();
        for _ in 0..1400 {
            sim.step_with(&inputs).unwrap();
        }
        let l1 = match sim.channel_val("lives-1") { Some(Val::Num(n)) => n, _ => 99.0 };
        let l2 = match sim.channel_val("lives-2") { Some(Val::Num(n)) => n, _ => 99.0 };
        assert!(l1 < 3.0 && l2 < 3.0, "both pilots hit independently: {} {}", l1, l2);
    }

    /// The full-stack card: piloted rig (raw axes -> vel-domain movement),
    /// focus, bombs (raw button + control-layer stock), boss hp phases via
    /// rules, spell-2 embedded. One scripted run hits every mechanism.
    #[test]
    #[ignore = "long full-card playthrough; run with cargo test --lib -- --ignored --test-threads=1"]
    fn reimu_vs_mima_plays() {
        // load_file resolves the card's imports (spell-2 + seal-orb come
        // from the translations)
        let mut sim = Sim::load_file(
            std::path::Path::new("../../cards/reimu_vs_mima.maku"),
            Some("reimu-vs-mima"),
        )
        .unwrap();
        let mut inputs = Inputs::default();
        let mut saw_needles = false;
        for k in 0..4500u64 {
            // net-zero wiggle with the raw axes; bomb once; focus mid-fight
            inputs.set_num("move-x", if k % 200 < 100 { 0.6 } else { -0.6 });
            inputs.set_flag("bomb", (900..930).contains(&k));
            inputs.set_flag("focus-firing", (400..600).contains(&k));
            sim.step_with(&inputs).unwrap();
            if !saw_needles {
                saw_needles = sim
                    .world
                    .entities
                    .iter()
                    .enumerate()
                    .any(|(i, _)| sim.world.sym_field_matches_at(i, "team", "player") && style(&sim, i).family == "gem");
            }
        }
        assert!(saw_needles, "focus switched the fire mode to needles");
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        let count = |n: &str| names.iter().filter(|x| x == &n).count();
        assert_eq!(count("spell"), 1, "non-spell broke into spell-2");
        assert_eq!(count("bomb"), 1, "one bomb consumed");
        assert_eq!(count("died"), 1, "boss down");
        // the piloted rig moved off its start: $player is entity-derived
        if let Some(Val::Pose(p)) = sim.channel_val("player") {
            let (x, y) = (p.x, p.y);
            assert!(x.abs() > 0.01 || (y + 3.0).abs() > 0.01, "rig integrated the axes");
        } else {
            panic!("no $player channel");
        }
        // field quiets after the kill (rig + parked guides only)
        assert!(live_count(&sim) <= 6, "post-fight field: {}", live_count(&sim));
    }

    /// The playable demo card exercises the whole gameplay layer at once:
    /// hostile spray hits/grazes, autofire kills drones.
    #[test]
    #[ignore = "long full-card playthrough; run with cargo test --lib -- --ignored --test-threads=1"]
    fn duel_card_plays() {
        let src = std::fs::read_to_string("../../cards/duel.maku").unwrap();
        let rig = crate::edn::stdlib("touhou").unwrap();
        let mut sim = Sim::load(&src, Some("duel")).unwrap();
        // the host layers the stock rig; boss/stage cards stay player-free
        sim.add_forms(&src, &format!("(defpattern __host-player-rig [] (player-rig))\n{}", rig)).unwrap();
        let mut inputs = Inputs::classic((0.0, -2.0), (0.0, -2.0));
        for k in 0..1200 {
            inputs.set_num("move-y", if k < 27 { 1.0 } else { 0.0 });
            sim.step_with(&inputs).unwrap();
        }
        assert!(sim.channel_u64("hits") > 0, "aimed spray reaches a stationary player");
        assert!(sim.channel_u64("graze") > 0, "fan neighbors graze");
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "died"),
            "autofire kills drones"
        );
    }

    /// F20: $nearest-enemy derives from :team :enemy entities when present.
    #[test]
    fn derived_nearest_enemy() {
        const CARD: &str = r#"
(import "touhou")
(defpattern hunt []
  (seq
    (enemy (pose c[2 3]) {:style {:family :dummy}})
    (wait (ticks 1))
    (bullet (vel p[3 (slew 720 90 (angle-of (- (live $nearest-enemy) pos)))])
                  {:style {:family :amulet}})))
"#;
        let mut sim = Sim::load(CARD, Some("hunt")).unwrap();
        for _ in 0..120 {
            sim.step().unwrap();
        }
        match sim.channel_val("nearest-enemy") {
            Some(Val::Pose(p)) => {
                let (x, y) = (p.x, p.y);
                assert!((x - 2.0).abs() < 1e-9 && (y - 3.0).abs() < 1e-9, "derived: {} {}", x, y);
            }
            v => panic!("bad channel: {:?}", v),
        }
        let (i, _) = sim
            .world
            .entities
            .iter()
            .enumerate()
            .find(|(i, _)| style(&sim, *i).family == "amulet")
            .unwrap();
        let sig = sim.ctx.sig.clone();
        let tau = sim.world.entity_tau(i, sim.world.tick);
        let readers = sim.motion_readers(i);
        let state = MotionState::new();
        let p = dyn_figure_pose_in(dyn_figure(&sim, i), tau, MotionEvalCtx::new(&state, &sig, &readers)).unwrap();
        assert!(p.x > 0.3, "homed toward derived enemy: {:?}", p);
    }

    /// Generational hot-swap: entities persist, program changes.
    #[test]
    fn swap_keeps_world() {
        const CARD: &str = r#"
(defpattern a [] (spawn (circle 6 (linear c[0.5 0]))))
"#;
        let mut sim = Sim::load(CARD, Some("a")).unwrap();
        for _ in 0..60 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.entities.len(), 6);
        sim.swap_forms(CARD, "(spawn (circle 3 (linear c[0.2 0])))").unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.entities.len(), 9, "old 6 keep flying + new 3");
        assert_eq!(sim.tick(), 61, "clock continues");
    }

    /// Layering starts on the ADD tick, not tick 0: a delayed add fires its
    /// pattern's timeline relative to when it was added.
    #[test]
    fn add_anchors_at_add_tick() {
        const CARD: &str = r#"
(defpattern a [] (dotimes [i inf :every (ticks 60)]
  (spawn (circle 2 (linear c[1 0])) {:style {:family :x}})))
(defpattern b [] (seq (wait (ticks 30))
  (spawn (circle 3 (linear c[1 0])) {:style {:family :y}})))
"#;
        let mut sim = Sim::load(CARD, Some("a")).unwrap();
        for _ in 0..100 {
            sim.step().unwrap();
        }
        sim.add_forms(CARD, "(b)").unwrap(); // added at tick 100
        for _ in 0..30 {
            sim.step().unwrap();
        }
        // b waits 30 ticks from ITS start: nothing through tick 129
        assert_eq!(live_family_count(&sim, "y"), 0);
        sim.step().unwrap(); // the step processing tick 130 = add(100) + 30
        let ys: Vec<_> = sim
            .world
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| style(&sim, *i).family == "y")
            .collect();
        assert_eq!(ys.len(), 3);
        assert_eq!(sim.world.entities.birth(ys[0].0), Some(130), "b's clock anchored at the add tick");
        // a kept its own cadence meanwhile (volleys at ticks 0, 60, 120)
        assert_eq!(live_family_count(&sim, "x"), 6);
    }

    /// Patterns are callable: (par (a) (b)) plays two patterns in parallel.
    #[test]
    fn parallel_patterns() {
        const CARD: &str = r#"
(defpattern a [n 4] (spawn (circle n (linear c[1 0])) {:style {:family :x}}))
(defpattern b [] (seq (wait 0.1) (spawn (circle 3 (linear c[2 0])) {:style {:family :y}})))
"#;
        let mut sim = Sim::load_forms(CARD, "(par (a) (b))").unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        let x = live_family_count(&sim, "x");
        let y = live_family_count(&sim, "y");
        assert_eq!((x, y), (4, 3), "both patterns ran in parallel");
    }

    /// Anonymous forms run with the card's defs in scope (the REPL path).
    #[test]
    fn load_forms_anonymous() {
        let card = r#"
(def spd 3.0)
(defpattern unused [] (spawn (circle 3 (linear c[1 0]))))
"#;
        let mut sim =
            Sim::load_forms(card, "(spawn (circle 8 (linear c[spd 0])))").unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.entities.len(), 8);
        let mut sim2 = Sim::load_forms(
            card,
            "(defpattern ring [n 5] (spawn (circle n (linear c[spd 0]))))",
        )
        .unwrap();
        sim2.step().unwrap();
        assert_eq!(sim2.world.entities.len(), 5);
    }

    #[test]
    fn lowered_closed_pt_oracle_card_step() {
        struct OracleGuard;
        impl Drop for OracleGuard {
            fn drop(&mut self) {
                crate::interp::set_oracle_for_tests(false);
            }
        }

        crate::interp::set_oracle_for_tests(true);
        let _guard = OracleGuard;
        const CARD: &str = r#"
(defpattern p []
  (spawn (polar (+ 2 (* 3 t) (sin (* 90 t)))
                (+ 15 (* 45 t) (cos (* 30 t))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..4 {
            sim.step().unwrap();
            assert_eq!(sim.render().len(), 1);
        }
    }

    #[test]
    fn unlowerable_user_fn_signal_falls_back() {
        const FALLBACK: &str = r#"
(defn twice [x] (* 2 x))
(defpattern p [] (spawn (cart (twice t) (+ 1 (* 3 t)))))
"#;
        const INLINE: &str = r#"
(defpattern p [] (spawn (cart (* 2 t) (+ 1 (* 3 t)))))
"#;
        let mut fallback = Sim::load(FALLBACK, Some("p")).unwrap();
        let mut inline = Sim::load(INLINE, Some("p")).unwrap();
        for _ in 0..5 {
            fallback.step().unwrap();
            inline.step().unwrap();
            let fallback_rows = fallback.render();
            let inline_rows = inline.render();
            assert_eq!(fallback_rows.len(), inline_rows.len());
            assert_render_rows_eq(&fallback_rows[0], &inline_rows[0]);
        }
    }

    /// F15 in the sim: 200's variant (axis 0, len 3) and color (axis 1 via
    /// explicit length 6) must bind to their axes, not the flat index.
    #[test]
    fn leading_axis_meta() {
        const CARD: &str = r#"
(defpattern axes []
  (spawn (map (fn [idx] ((rot m"15*idx") (circle 6 (linear c[1 0]))))
              (iota 3))
         {:style {:family :x
                  :variant [:b :c :w]
                  :color (nth [:blue :green :teal] (iota 6))}}))
"#;
        let mut sim = Sim::load(CARD, Some("axes")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.entities.len(), 18);
        let b = |k: usize| style(&sim, k);
        assert_eq!(b(0).variant, "b");
        assert_eq!(b(6).variant, "c");
        assert_eq!(b(12).variant, "w");
        assert_eq!(b(0).color, "blue");
        assert_eq!(b(3).color, "blue"); // cycles within the ring axis
        assert_eq!(b(7).color, "green");
    }

    /// Dyn-valued top-level numeric fields are evaluated into SoA fields;
    /// rules/colliders read those fields like any other entity meta.
    #[test]
    fn dyn_numeric_meta_fields() {
        const CARD: &str = r#"
(deftick
  (map (fn [e]
         (render {:shape :point
                  :scale e.scale
                  :alpha e.opacity
                  :theta e.facing}))
       (entities-where {:render :sprite})))
(defpattern tags []
  (spawn (still)
         {:render :sprite
          :scale (+ 1 t) :opacity (- 1 (* 0.5 t)) :facing (* 90 t)}))
"#;
        let mut sim = Sim::load(CARD, Some("tags")).unwrap();
        for _ in 0..120 {
            sim.step().unwrap(); // t = 1s
        }
        let RenderData::Point { theta: th, scale, alpha, .. } = &sim.render()[0].data else {
            panic!("expected a dot");
        };
        assert!((scale - 2.0).abs() < 0.02, "scale(1s) = 2: {}", scale);
        assert!((alpha - 0.5).abs() < 0.02, "opacity(1s) = 0.5: {}", alpha);
        assert!((th - 90.0).abs() < 1.0, "facing(1s) = 90°: {}", th);

        // collision: a bullet whose base radius misses the player connects
        // once :scale grows the collider.
        const HIT: &str = r#"
(import "touhou")
(defpattern rig []
  (let [p (spawn (live $player)
                 (circle-collider {:layer :player-hurt :r 0.06})
                 {:team :player-body
                  :graze 0 :hits 0})]
    (let [body (nth p 0)]
      (bind-channel! $graze (:graze body))
      (bind-channel! $hits (:hits body)))))
(defpattern scaled [s 1]
  (par (rig)
       (spawn ((pose c[0.5 0]) (still))
              (circle-collider {:layer :damage :r 0.1})
              {:scale s})))
"#;
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        let mut near = Sim::load_forms(HIT, "(scaled 1)").unwrap();
        for _ in 0..10 {
            near.step_with(&inputs).unwrap();
        }
        assert_eq!(near.channel_u64("hits"), 0, "base radius misses at 0.5");
        let mut big = Sim::load_forms(HIT, "(scaled 6)").unwrap();
        for _ in 0..10 {
            big.step_with(&inputs).unwrap();
        }
        assert_eq!(big.channel_u64("hits"), 1, "scaled collider connects");
    }

    #[test]
    fn laser_opts_seed_entity_fields() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (curve {:warn 0.5 :active 2 :u-max 6})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "warn"), Some(0.5));
        assert_eq!(sim.world.col_get_at(0, "active"), Some(2.0));
        assert_eq!(sim.world.col_get_at(0, "u-max"), Some(6.0));
    }

    #[test]
    fn element_seeds_override_spawn_meta() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (curve {:warn 1}) {:warn 2}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "warn"), Some(1.0));
    }

    #[test]
    fn dyn_seed_refreshes_per_tick() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (fields (pose c[0 0]) {:grow m"2*t"})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..60 {
            sim.step().unwrap();
        }
        let grow = sim.world.col_get_at(0, "grow").unwrap();
        assert!((grow - 1.0).abs() < 0.02, "grow at 0.5s = 1: {}", grow);
    }

    #[test]
    fn fields_on_pose_figure_and_frame_transparency() {
        const CARD: &str = r#"
(defpattern p []
  (spawn ((rot 90) (fields (pose c[1 0]) {:tag :abc}))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert!(sim.world.sym_field_matches_at(0, "tag", "abc"));
        let p = dyn_figure_pose(dyn_figure(&sim, 0), 0.0, &MotionState::new(), &SigEnv::default()).unwrap();
        assert!(p.x.abs() < 1e-9 && (p.y - 1.0).abs() < 1e-9, "rotated pose: {:?}", p);
    }

    #[test]
    fn constructor_trailing_map() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (linear c[0 1] {:speed 3})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "speed"), Some(3.0));
    }

    #[test]
    fn distribution_over_repeat() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (circle 3 (curve {:warn 1 :active 1}))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.entities.len(), 3);
        for i in 0..3 {
            assert_eq!(sim.world.col_get_at(i, "warn"), Some(1.0));
        }
    }

    /// A shared array-valued meta SIGNAL binds per element like a static
    /// axis array: each entity's field is one scalar lane per tick.
    #[test]
    fn dyn_meta_arrays_bind_per_element() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (circle 3 (linear p[1 0])) {:hue m"100*iota(3) + t"}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.entities.len(), 3);
        let tau = 1.0 / DEFAULT_TICK_RATE;
        for i in 0..3 {
            let hue = sim.world.col_get_at(i, "hue").unwrap();
            let want = 100.0 * i as f64 + tau;
            assert!((hue - want).abs() < 1e-9, "entity {i}: hue {hue}, want {want}");
        }
    }

    /// §8 scope semantics under the guard-unwind rule: cancellation kills
    /// the scope, and the TASK CONTINUES after it — (seq (until p a) b)
    /// reaches b.
    #[test]
    fn until_cancels_scope_not_task() {
        const CARD: &str = r#"
(defpattern uc []
  (seq
    (defcell stop 0)
    (fork (seq (wait 0.1) (set! stop 1)))
    (until (> stop 0)
      (for [i inf :every (ticks 2)]
        (spawn (still))))
    (event :after-until)))
"#;
        let mut sim = Sim::load(CARD, Some("uc")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        let n = sim.world.entities.len();
        assert!((5..=8).contains(&n), "spawner ran ~0.1s then died: {}", n);
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "after-until"),
            "the task resumed after the cancelled scope"
        );
    }

    #[test]
    fn finally_runs_on_completion() {
        const CARD: &str = r#"
(defpattern f []
  (seq
    (finally
      (seq (event :a) (wait (ticks 5)))
      (event :cleanup))
    (event :after)))
"#;
        let mut sim = Sim::load(CARD, Some("f")).unwrap();
        for _ in 0..12 {
            sim.step().unwrap();
        }
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        let a = names.iter().position(|n| n == "a");
        let cleanup = names.iter().position(|n| n == "cleanup");
        let after = names.iter().position(|n| n == "after");
        assert!(a.is_some(), "body started");
        assert!(cleanup.is_some(), "cleanup ran after body completion");
        assert!(after.is_some(), "sequence continued after cleanup");
        assert!(a < cleanup && cleanup < after, "event order: {:?}", names);
    }

    #[test]
    fn finally_runs_on_cancellation() {
        const CARD: &str = r#"
(defpattern f []
  (seq
    (defcell stop 0)
    (fork (seq (wait 0.05) (set! stop 1)))
    (until (> stop 0)
      (finally
        (seq (event :body-start) (wait 999) (event :body-late))
        (event :cleanup)))
    (event :after)))
"#;
        let mut sim = Sim::load(CARD, Some("f")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        assert!(names.iter().any(|n| n == "body-start"));
        assert!(names.iter().any(|n| n == "cleanup"), "cleanup ran on cancellation");
        assert!(names.iter().any(|n| n == "after"), "outer seq resumed");
        assert!(!names.iter().any(|n| n == "body-late"), "cancelled body did not continue");
    }

    /// New capability: a fork killed by an inherited guard still runs
    /// protected cleanup before the task ends.
    #[test]
    fn finally_runs_when_fork_dies() {
        const CARD: &str = r#"
(defpattern f []
  (seq
    (defcell p 0)
    (fork (seq (wait 0.05) (set! p 1)))
    (until (> p 0)
      (fork (finally
        (seq (wait 999))
        (event :fork-cleanup)))
      (wait 999))))
"#;
        let mut sim = Sim::load(CARD, Some("f")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "fork-cleanup"),
            "fork cleanup ran after inherited guard killed the task"
        );
    }

    #[test]
    fn race_first_wins() {
        const CARD: &str = r#"
(defpattern r []
  (seq
    (race
      (seq (wait (ticks 3)) (event :fast))
      (seq (wait (ticks 100)) (event :slow)))
    (event :after)))
"#;
        let mut sim = Sim::load(CARD, Some("r")).unwrap();
        for _ in 0..20 {
            sim.step().unwrap();
        }
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        let fast = names.iter().position(|n| n == "fast");
        let after = names.iter().position(|n| n == "after");
        assert!(fast.is_some(), "fast arm won");
        assert!(!names.iter().any(|n| n == "slow"), "slow arm was cancelled");
        assert!(after.is_some() && fast < after, "parent resumed after win: {:?}", names);
    }

    #[test]
    fn race_loser_cleanup() {
        const CARD: &str = r#"
(defpattern r []
  (race
    (seq (wait (ticks 3)) (event :fast))
    (finally
      (seq (wait (ticks 100)) (event :slow))
      (event :slow-cleanup))))
"#;
        let mut sim = Sim::load(CARD, Some("r")).unwrap();
        for _ in 0..20 {
            sim.step().unwrap();
        }
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        assert!(names.iter().any(|n| n == "fast"), "fast arm won");
        assert!(names.iter().any(|n| n == "slow-cleanup"), "loser cleanup ran");
        assert!(!names.iter().any(|n| n == "slow"), "loser body did not finish");
    }

    /// The states FSM: routing goto skips states, a timeout expressed as
    /// body code (fork + wait + bare goto) ends a looping body, finalizers
    /// run on the way out, and fall-through completes the machine.
    #[test]
    fn states_trampoline() {
        const CARD: &str = r#"
(defpattern m []
  (seq
    (states
      (:opening (goto :b))
      (:a (spawn (circle 3 (still))))            ; skipped by the goto
      (:b
        (finally
          (seq
            (fork (seq (wait 0.05) (goto)))      ; timeout: exit to successor
            (for [i inf :every 1] (spawn (circle 5 (still)))))
          (event :b-done))))
    (event :machine-done)))
"#;
        let mut sim = Sim::load(CARD, Some("m")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.entities.len(), 5, ":opening routed straight to :b");
        for _ in 0..20 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.entities.len(), 5, "the :b loop died at the timeout");
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        let b_done = names.iter().position(|n| n == "b-done");
        let m_done = names.iter().position(|n| n == "machine-done");
        assert!(b_done.is_some(), "finalizer ran on timeout exit");
        assert!(m_done.is_some(), "falling off the end completed the machine");
        assert!(b_done < m_done, "finalizer before machine completion");
    }

    /// goto is a scoped exit: from a fork inside the state body it cancels
    /// the whole state scope — including a nested (until …) guard the body
    /// wrapped itself in — and re-enters at the target label.
    #[test]
    fn states_goto_from_fork_and_until() {
        const CARD: &str = r#"
(defpattern m []
  (seq
    (defcell hp 10)
    (export hp)
    (states
      (:spell
        (finally
          (seq
            (fork (seq (wait 0.05) (goto :post)))
            (until (<= $hp 0)                    ; the hp gate, as body code
              (for [i inf :every (ticks 2)] (spawn (still)))))
          (event :spell-out)))
      (:post (event :post)))))
"#;
        let mut sim = Sim::load(CARD, Some("m")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        let n = sim.world.entities.len();
        assert!((3..=6).contains(&n), "spawner died at the goto: {}", n);
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        assert!(names.iter().any(|n| n == "spell-out"), "finalizer ran on goto exit");
        assert!(names.iter().any(|n| n == "post"), "re-entered at the target label");
    }

    /// Labels are values: computed goto routing makes the machine a Markov
    /// chain (here over the deterministic world rng).
    #[test]
    fn states_markov_routing() {
        const CARD: &str = r#"
(defpattern m []
  (states
    (:a (event :in-a)
        (wait (ticks 4))
        (goto (nth [:a :b] (rand-int 0 2))))
    (:b (event :in-b)
        (wait (ticks 4))
        (goto (nth [:a :b] (rand-int 0 2))))))
"#;
        let mut sim = Sim::load(CARD, Some("m")).unwrap();
        for _ in 0..200 {
            sim.step().unwrap();
        }
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        let a = names.iter().filter(|n| *n == "in-a").count();
        let b = names.iter().filter(|n| *n == "in-b").count();
        assert!(a + b >= 40, "the chain kept walking: {} + {}", a, b);
        assert!(a > 0 && b > 0, "both states visited: a={} b={}", a, b);
    }

    /// The `phases` sugar over `states` — a touhou.maku MACRO now: clause
    /// opts desugar to body code at macro time — :timeout to a fork racing
    /// the body, :until to an until wrapper (a bare wait-for when the
    /// clause has no body).
    #[test]
    fn phases_sugar_desugars() {
        const CARD: &str = r#"
(import "touhou")
(defpattern m []
  (phases
    (:spell {:timeout 0.05}
      (for [i inf :every 1] (spawn (circle 4 (still))))
      (finally (event :spell-out)))
    (:end (event :end))))
"#;
        let mut sim = Sim::load(CARD, Some("m")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.entities.len(), 4, ":timeout ended the spell loop");
        let has = |sim: &Sim, n: &str| {
            sim.events_vec().iter().any(|e| &*e.name == n)
        };
        assert!(has(&sim, "spell-out"), "finalizer ran on the timeout path");
        assert!(has(&sim, "end"), "fell through to the next phase");

        const CARD2: &str = r#"
(import "touhou")
(defpattern m []
  (seq
    (defcell hp 5)
    (export hp)
    (fork (seq (wait 0.2) (set! hp 0)))
    (phases
      (:gate {:until (<= $hp 0)})
      (:end (event :end)))))
"#;
        let mut sim2 = Sim::load(CARD2, Some("m")).unwrap();
        for _ in 0..12 {
            sim2.step().unwrap();
        }
        assert!(!has(&sim2, "end"), ":until (empty body) still gating");
        for _ in 0..24 {
            sim2.step().unwrap();
        }
        assert!(has(&sim2, "end"), ":until released the gate when the channel dropped");
    }

    /// The states machine as a player-control FSM: ground/air zones with
    /// per-state movesets (forked in the body, dying with the state) and
    /// transitions driven by an input channel.
    #[test]
    fn states_player_control() {
        const CARD: &str = r#"
(defpattern pc []
  (states
    (:ground
      (fork (for [i inf :every (ticks 2)]
              (spawn (still) {:style {:family :circle}})))
      (wait-for (> $jump 0.5))
      (goto :air))
    (:air
      (fork (for [i inf :every (ticks 2)]
              (spawn (still) {:style {:family :star}})))
      (wait-for (< $jump 0.5))
      (goto :ground))))
"#;
        let mut sim = Sim::load(CARD, Some("pc")).unwrap();
        let mut inp = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        inp.set("jump", Val::Num(0.0));
        for _ in 0..20 {
            sim.step_with(&inp).unwrap();
        }
        let count = |sim: &Sim, fam: &str| {
            sim.world.entities.iter().enumerate().filter(|(i, _)| {
                sim.world.entities.is_alive(*i) && style(&sim, *i).family == fam
            }).count()
        };
        let g1 = count(&sim, "circle");
        assert!(g1 >= 8, "ground moveset firing: {}", g1);
        assert_eq!(count(&sim, "star"), 0, "air moveset dormant on the ground");
        inp.set("jump", Val::Num(1.0));
        for _ in 0..20 {
            sim.step_with(&inp).unwrap();
        }
        assert_eq!(count(&sim, "circle"), g1, "ground moveset died on takeoff");
        let a1 = count(&sim, "star");
        assert!(a1 >= 8, "air moveset firing: {}", a1);
        inp.set("jump", Val::Num(0.0));
        for _ in 0..20 {
            sim.step_with(&inp).unwrap();
        }
        assert_eq!(count(&sim, "star"), a1, "air moveset died on landing");
        assert!(count(&sim, "circle") > g1, "ground moveset resumed");
    }

    /// goto outside any state machine is an error, and machines are
    /// lexically scoped: a pattern invoked from a state body has no
    /// enclosing machine in ITS text, so its goto fails too.
    #[test]
    fn goto_scoping() {
        let mut sim =
            Sim::load_forms("(defpattern p [] (still))", "(goto :anywhere)").unwrap();
        assert!(sim.step().is_err(), "goto outside a machine errors");

        const CARD: &str = r#"
(defpattern callee [] (goto :a))
(defpattern m []
  (states
    (:a (callee))))
"#;
        let mut sim2 = Sim::load(CARD, Some("m")).unwrap();
        assert!(
            sim2.step().is_err(),
            "called patterns don't inherit the machine scope (goto is lexical)"
        );
    }

    /// spawn takes several meta maps merged per-key, later wins — the hook
    /// library templates use: (spawn d {defaults…} user-meta).
    #[test]
    fn spawn_meta_merges() {
        const CARD: &str = r#"
(defpattern p []
  (spawn (pose c[0 0])
         {:team :enemy :hp 5 :a 1 :style {:family :gem}}
         {:hp 2 :style {:family :star :color :red}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(2.0), "later map wins per-key");
        assert_eq!(sim.world.col_get_at(0, "a"), Some(1.0), "earlier keys survive");
        assert!(sim.world.sym_field_matches_at(0, "team", "enemy"));
        assert_eq!(style(&sim, 0).family, "star", ":style replaces wholesale");
    }

    /// $tick: the world clock as a channel — what lets deadline columns
    /// (iframe-until) be written by library code instead of engine verbs.
    #[test]
    fn tick_channel() {
        const CARD: &str = r#"
(defpattern p []
  (seq (wait-for (>= $tick 5)) (spawn (pose c[0 0]))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..4 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.entities.len(), 0, "gate still closed");
        for _ in 0..4 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.entities.len(), 1, "gate opened at tick 5");
    }

    #[test]
    fn change_col_updates_accumulate_at_next_tick_boundary() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]) {:hp 10})]
    (let [b (first bs)]
      (seq
        (change-col b :hp (fn [hp] (- (value-or hp 0) 3)))
        (change-col b :hp (fn [hp] (- hp 4)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(10.0), "same tick reads pre-write value");
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(3.0), "both queued decrements compose");
    }

    #[test]
    fn change_col_same_tick_reads_see_pre_tick_value() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]) {:hp 5})]
    (let [b (first bs)]
      (seq
        (change-col b :hp (fn [hp] (+ hp 4)))
        (set-col b :seen (:hp b))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(5.0));
        assert_eq!(sim.world.col_get_at(0, "seen"), None);
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(9.0));
        assert_eq!(sim.world.col_get_at(0, "seen"), Some(5.0));
    }

    #[test]
    fn change_col_order_is_deterministic() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]) {:hp 1})]
    (let [b (first bs)]
      (seq
        (change-col b :hp (fn [hp] (* hp 2)))
        (change-col b :hp (fn [hp] (+ hp 3)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(5.0));
    }

    #[test]
    fn change_col_to_dead_entity_is_dropped() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]) {:hp 10})]
    (let [b (first bs)]
      (seq
        (change-col b :hp (fn [hp] (- hp 9)))
        (cull b)))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert!(!sim.world.entities.is_alive(0));
        sim.step().unwrap();
        assert!(!sim.world.entities.is_alive(0));
        assert!(sim.world.pending_writes.is_empty());
    }

    #[test]
    fn change_col_update_fn_cannot_read_channels() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]) {:hp 1})]
    (let [b (first bs)]
      (change-col b :hp (fn [hp] (+ hp $rank))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let err = sim.step().unwrap_err();
        assert!(err.contains("host does not provide channel $rank"), "{err}");
    }

    #[test]
    fn set_col_sugar_queues_constant_update() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]) {:hp 1})]
    (let [b (first bs)]
      (set-col b :hp 7))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(1.0));
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(7.0));
    }

    #[test]
    fn remat_field_only_leaves_motion_epoch_untouched() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (linear c[120 0]) {:hp 1})]
    (remat (first bs) {:hp 5})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(1.0));
        assert_eq!(sim.world.entity_motion_tau(0, sim.world.tick), 1.0 / DEFAULT_TICK_RATE);
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(5.0));
        assert_eq!(sim.world.entity_motion_tau(0, sim.world.tick), 2.0 / DEFAULT_TICK_RATE);
        let p = sim.world.entities.sampled_pose(0, sim.world.tick - 1).unwrap();
        assert!((p.x - 1.0).abs() < 1e-9 && p.y.abs() < 1e-9, "field-only remat moved discontinuously: {p:?}");
    }

    #[test]
    fn remat_motion_restarts_motion_but_retains_fields_and_entity_age() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (linear c[120 0]) {:hp 7})]
    (remat (first bs) (linear c[0 120]))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(7.0));
        assert_eq!(sim.world.entity_tau(0, sim.world.tick), 2.0 / DEFAULT_TICK_RATE);
        assert_eq!(sim.world.entity_motion_tau(0, sim.world.tick), 1.0 / DEFAULT_TICK_RATE);
        let p = sim.world.entities.sampled_pose(0, sim.world.tick - 1).unwrap();
        assert!((p.x - 1.0).abs() < 1e-9 && p.y.abs() < 1e-9, "motion remat did not anchor at exit: {p:?}");
        sim.step().unwrap();
        let p = sim.world.entities.sampled_pose(0, sim.world.tick - 1).unwrap();
        assert!((p.x - 1.0).abs() < 1e-9 && (p.y - 1.0).abs() < 1e-9, "new motion did not start at tau 0: {p:?}");
    }

    #[test]
    fn remat_combined_map_applies_motion_and_fields_at_one_boundary() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (linear c[120 0]) {:hp 1})]
    (remat (first bs) {:motion (linear c[0 120]) :hp (fn [hp] (+ hp 2))})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(3.0));
        assert_eq!(sim.world.entity_motion_tau(0, sim.world.tick), 1.0 / DEFAULT_TICK_RATE);
        let p = sim.world.entities.sampled_pose(0, sim.world.tick - 1).unwrap();
        assert!((p.x - 1.0).abs() < 1e-9 && p.y.abs() < 1e-9, "combined remat boundary pose: {p:?}");
    }

    #[test]
    fn remat_and_change_col_share_push_order() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]) {:hp 1})]
    (let [b (first bs)]
      (seq
        (change-col b :hp (fn [hp] (+ hp 10)))
        (remat b {:hp 5})))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(5.0));
    }

    #[test]
    fn remat_to_dead_entity_is_dropped() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]) {:hp 1})]
    (let [b (first bs)]
      (seq
        (remat b {:hp 5})
        (cull b)))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert!(!sim.world.entities.is_alive(0));
        sim.step().unwrap();
        assert!(sim.world.pending_writes.is_empty());
    }

    #[test]
    fn remat_direct_multi_element_figure_errors_at_action_exec() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (pose c[0 0]))]
    (remat (first bs) (circle 2 (still)))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        let err = sim.step().unwrap_err();
        assert!(err.contains("expected dyn pose"), "{err}");
        assert!(sim.world.pending_writes.is_empty(), "invalid remat must not queue");
    }

    #[test]
    fn remat_reads_later_in_same_tick_see_old_values() {
        const CARD: &str = r#"
(defpattern p []
  (let [bs (spawn (linear c[120 0]) {:hp 1})]
    (let [b (first bs)]
      (seq
        (remat b {:motion (linear c[0 120]) :hp 5})
        (set-col b :seen (:hp b))
        (set-col b :seen-x (:x (:pos b)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(1.0));
        assert_eq!(sim.world.col_get_at(0, "seen"), None);
        sim.step().unwrap();
        assert_eq!(sim.world.col_get_at(0, "hp"), Some(5.0));
        assert_eq!(sim.world.col_get_at(0, "seen"), Some(1.0));
        assert_eq!(sim.world.col_get_at(0, "seen-x"), Some(0.0));
    }

/// Variadic macros (& rest) + macro-time form processing: a macro that
/// walks its clause list with map/nth and splices the transforms — the
/// mechanism the stdlib's `phases` is built from.
#[test]
fn variadic_macro_processes_clauses() {
    const CARD: &str = r#"
(defmacro spawn-each [& specs]
  `(par ~@(map (fn [c] `(spawn (linear c[~(nth c 1) 0]))) specs)))
(defpattern p []
  (spawn-each (:a 1) (:b 2) (:c 3)))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    assert_eq!(sim.world.entities.len(), 3, "one spawn per clause");
}

/// Quasiquote must walk map values too: genre helpers need variadic macro
/// rest args as ordinary macro-time values inside collider/meta maps.
#[test]
fn variadic_macro_unquotes_inside_maps() {
    const CARD: &str = r#"
(defn meta-hitbox [metas default]
  (match metas
    [] default
    [m & rest] (let [later (meta-hitbox rest default)
                     here (get m :hitbox)]
                 (if (nothing? here) later here))))
(defmacro bullet [dyn & metas]
  `(spawn ~dyn
          (circle-collider {:layer :damage :r ~(meta-hitbox metas 0.12)})
          ~@metas))
(defpattern p []
  (bullet (pose c[0 0]) {:hitbox 0.4}))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    let row = sim
        .world
        .entities
        .iter()
        .enumerate()
        .find(|(i, _)| sim.world.entities.is_alive(*i))
        .map(|(i, _)| i)
        .unwrap();
    let projector = sim.world.entities.collider_projector(row).unwrap();
    let tick_rate = sim.world.tick_rate();
    let mut slots = Vec::new();
    crate::sim::slots::materialize_collider_defs_into(
        projector,
        0.0,
        &MotionState::new(),
        &SigEnv::default(),
        None,
        None,
        &mut sim.world.symbols,
        &mut slots,
        tick_rate,
    )
    .unwrap();
    let (_, radius) = slots.iter().find_map(|slot| match slot.slot().shape {
        ColliderSlotShape::Circle { ref radius } => Some((slot, radius)),
        _ => None,
    }).unwrap();
    let r = eval_dyn(radius, 0.0, &MotionState::new(), &SigEnv::default()).unwrap();
    assert!((r - 0.4).abs() < 1e-6, "rest arg helper unquoted into map");
}

/// `when`/`unless` are prelude macros now (autoimported): a false
/// condition means the no-op action, in any action position.
#[test]
fn prelude_when_unless() {
    const CARD: &str = r#"
(defpattern p []
  (for [i 4 :every (ticks 1)]
    (when (= (mod i 2) 0) (spawn (pose c[0 0])))
    (unless true (spawn (pose c[9 9])))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..8 {
        sim.step().unwrap();
    }
    assert_eq!(sim.world.entities.len(), 2, "even iterations only");
}

/// boss owns the boss conventions: map-valued boss state is bound
/// for the host, the machine is held until the boss registers, `boss` is
/// bound for the body, and phases' {:hp n} gate reads local boss hp.
#[test]
fn spawn_boss_owns_conventions() {
    const CARD: &str = r#"
(import "touhou")
(defchannel $m {:hp 0})
(defpattern m []
  (boss $m (pose c[0 2])
              {:hp 3 :hitbox 0.4 :style {:family :lstar}}
    (phases
      (:one {:hp 1} (seq (event :phase-one) (wait 99))
        (finally (event :one-out) (invuln (nth boss 0) 0.1)))
      (:two {:hp 0} (seq (event :phase-two) (wait 99))))))
"#;
    let mut sim = Sim::load(CARD, Some("m")).unwrap();
    for _ in 0..4 {
        sim.step().unwrap();
    }
    let has = |sim: &Sim, n: &str| {
        sim.events_vec().iter().any(|e| &*e.name == n)
    };
    assert!(has(&sim, "phase-one"), "machine started once local boss hp registered");
    assert!(!has(&sim, "phase-two"), "hp gate holds while hp > 1");
    assert!(
        matches!(sim.channel_val("m"), Some(Val::Map(kvs)) if matches!(map_get(&Val::Map(kvs.clone()), "hp"), Some(Val::Num(n)) if n == 3.0)),
        "public boss channel is a map with hp"
    );
    // knock hp down: the exposure publishes, the gate releases
    sim.world.col_set_at(0, &"hp".into(), 1.0);
    for _ in 0..4 {
        sim.step().unwrap();
    }
    assert!(has(&sim, "one-out"), "finally ran at the phase edge");
    assert!(has(&sim, "phase-two"), "hp gate released into the next phase");
}

/// (bind-channel! $name expr): instance-scoped derived channels can close
/// over handles and cells, overriding a top-level defchannel default.
#[test]
fn bind_channel_closes_over_handles_and_cells() {
    const CARD: &str = r#"
(import "touhou")
(defchannel $dummy {:hp 0 :phase :none})
(defpattern p []
  (seq
    (defcell phase :warmup)
    (let [e (enemy (pose c[0 2]) {:hp 5})]
      (let [b (nth e 0)]
        (bind-channel! $dummy {:hp (:hp b) :phase phase})
        (wait (ticks 2))
        (set! phase :main)
        (set-col b :hp 2)))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    assert!(
        matches!(sim.channel_val("dummy"), Some(Val::Map(kvs)) if
            matches!(map_get(&Val::Map(kvs.clone()), "hp"), Some(Val::Num(n)) if n == 5.0) &&
            matches!(map_get(&Val::Map(kvs.clone()), "phase"), Some(Val::Kw(k)) if &*k == "warmup"))
    );
    for _ in 0..4 {
        sim.step().unwrap();
    }
    assert!(
        matches!(sim.channel_val("dummy"), Some(Val::Map(kvs)) if
            matches!(map_get(&Val::Map(kvs.clone()), "hp"), Some(Val::Num(n)) if n == 2.0) &&
            matches!(map_get(&Val::Map(kvs.clone()), "phase"), Some(Val::Kw(k)) if &*k == "main"))
    );
}

/// (defchannel $name expr): card-defined derived channels, recomputed
/// each tick — the stdlib's $enemies/$nearest-enemy are these; a custom
/// one composes engine channels and world queries freely.
#[test]
fn defchannel_derives_per_tick() {
    const CARD: &str = r#"
(import "touhou")
(defchannel $reds (count-entities {:team :enemy :color :red}))
(defpattern p []
  (seq
    (enemy (pose c[0 2]) {:style {:family :circle :color :red}})
    (enemy (pose c[1 2]) {:style {:family :circle :color :blue}})
    (wait-for (>= $reds 1))
    (spawn (pose c[0 -3]) {:marker 1})))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..3 {
        sim.step().unwrap();
    }
    // $enemies (stdlib defchannel) counts both; $reds (card) counts one
    assert!(matches!(sim.ctx.sig.channel("enemies"), Some(Val::Num(n)) if n == 2.0));
    assert!(matches!(sim.ctx.sig.channel("reds"), Some(Val::Num(n)) if n == 1.0));
    assert!(
        sim.world
            .entities
            .iter()
            .enumerate()
            .any(|(i, _)| sim.world.col_get_at(i, "marker").is_some()),
        "control layer saw the derived channel"
    );
    // $nearest-enemy now derives from the stdlib defchannel
    assert!(matches!(sim.ctx.sig.channel("nearest-enemy"), Some(Val::Pose(_))));
}

#[test]
fn entity_sets_broadcast_keyword_accessors() {
    const CARD: &str = r#"
(defchannel $enemy-hp-view (:hp (entities-where (matches :team :enemy))))
(defchannel $enemy-pos-view (:pos (entities-where (matches :team :enemy :color :red))))
(defpattern p []
  (seq
    (spawn (pose c[2 3]) {:team :enemy :hp 4 :style {:color :red}})
    (spawn (pose c[-1 0]) {:team :enemy :hp 6 :style {:color :blue}})
    (wait (ticks 1))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..2 {
        sim.step().unwrap();
    }
    // Entity sets are ephemeral row-index views, not stable per-entity tables.
    let Some(Val::Arr(hp)) = sim.ctx.sig.channel("enemy-hp-view") else {
        panic!("expected hp array")
    };
    assert_eq!(hp.len(), 2);
    assert!(matches!((&hp[0], &hp[1]), (Val::Num(a), Val::Num(b)) if *a == 4.0 && *b == 6.0));
    let Some(Val::Pose(p)) = sim.ctx.sig.channel("enemy-pos-view") else {
        panic!("expected single red enemy position")
    };
    assert!((p.x - 2.0).abs() < 1e-9 && (p.y - 3.0).abs() < 1e-9);
}

#[test]
fn predicate_queries_drive_manip() {
    const CARD: &str = r#"
(defpattern p []
  (seq
    (spawn (pose c[0 0]) {:team :enemy :hp 4 :style {:color :red}})
    (spawn (pose c[0 0]) {:team :enemy :hp 6 :style {:color :blue}})
    (manip (matches :team :enemy :color :red)
      (fn [b] (set-col b :hp 1)))
    (wait (ticks 1))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..2 {
        sim.step().unwrap();
    }
    let hp = sim
        .world
        .entities
        .iter()
        .enumerate()
        .map(|(i, _)| sim.world.col_get_at(i, "hp").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(hp, vec![1.0, 6.0]);
}

#[test]
fn keyword_metadata_fields_query_and_access() {
    const CARD: &str = r#"
(defchannel $boss-role (:role (entities-where (matches :role :boss))))
(defchannel $elite-count (count-entities {:role :elite}))
(defpattern p []
  (seq
    (spawn (pose c[0 0]) {:team :enemy :role :boss :hp 4})
    (spawn (pose c[1 0]) {:team :enemy :role :elite :hp 6})
    (manip (matches :role :boss)
      (fn [b] (set-col b :hp 1)))
    (wait (ticks 1))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..2 {
        sim.step().unwrap();
    }
    assert!(matches!(sim.ctx.sig.channel("boss-role"), Some(Val::Kw(k)) if &*k == "boss"));
    assert!(matches!(sim.ctx.sig.channel("elite-count"), Some(Val::Num(n)) if n == 1.0));
    assert_eq!(sim.world.col_get_at(0, "hp"), Some(1.0));
    assert_eq!(sim.world.col_get_at(1, "hp"), Some(6.0));
}

#[test]
fn structured_meta_is_not_retained_as_entity_fields() {
    const CARD: &str = r#"
(defpattern p []
  (spawn (pose c[0 0])
         {:team :enemy
          :radius 0.2
          :collision {:radius 0.3}
          :tags [:a :b]}))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    assert_eq!(sim.world.col_get_at(0, "radius"), Some(0.2));
    assert_eq!(sim.world.col_get_at(0, "collision"), None);
    assert_eq!(sim.world.col_get_at(0, "tags"), None);
    let collision = sim.world.symbols.lookup("collision");
    assert!(
        collision
            .and_then(|field| sim.world.sym_field_value_at(0, field))
            .is_none()
    );
}

#[test]
fn entity_rows_do_not_shift_after_cull() {
    const CARD: &str = r#"
(defchannel $enemy-rows (entities-where (matches :team :enemy)))
(defpattern p []
  (seq
    (let [a (spawn (pose c[0 0]) {:team :enemy :hp 1})
          b (spawn (pose c[1 0]) {:team :enemy :hp 2})]
      (seq
        (wait (ticks 2))
        (cull (first a))
        (wait (ticks 1))))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..2 {
        sim.step().unwrap();
    }
    let Some(Val::EntitySet(rows)) = sim.ctx.sig.channel("enemy-rows") else {
        panic!("expected row view before cull")
    };
    assert_eq!(&*rows, &[0, 1]);
    for _ in 0..3 {
        sim.step().unwrap();
    }
    let Some(Val::EntitySet(rows)) = sim.ctx.sig.channel("enemy-rows") else {
        panic!("expected row view after cull")
    };
    assert_eq!(&*rows, &[1]);
    assert!(!sim.world.entities.is_alive(0));
    assert!(sim.world.entities.is_alive(1));
}

#[test]
fn sited_evolve_advances_once_per_tick_and_persists() {
    // An evolve inside a per-tick re-evaluated vel component: state lives
    // at the ScanSite, advances once per boundary, and within-tick reads
    // see the settled value (evolve-reexpression-design.md milestone 1).
    const CARD: &str = r#"
(defpattern p []
  (spawn (vel c[(evolve 0 (fn [s c] (+ s (* 60 (:dt c))))) 0])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    let schema = sim.world.entities.motion_schema(0).unwrap();
    let site = schema
        .val_keys
        .iter()
        .copied()
        .find(|key| matches!(key, MotionStateKey::ScanSite { .. }))
        .expect("sited evolve interns a val ScanSite slot");
    for _ in 0..2 {
        sim.step().unwrap();
    }
    let cell = sim.world.entities.state_val(0, site).unwrap();
    assert_eq!(cell.tick, 3, "one advance per tick");
    let Val::Num(vx) = cell.state else { panic!("numeric evolve state, got {:?}", cell.state) };
    let expected = 3.0 * 60.0 / DEFAULT_TICK_RATE;
    assert!((vx - expected).abs() < 1e-9, "ramped velocity state: {vx} (expected {expected})");
}

#[test]
fn sited_evolve_reproduces_slew() {
    // The prelude slew macro must match a hand-written sited evolve tick
    // for tick (the step-3 re-expression contract).
    const CARD: &str = r#"
(defpattern p []
  (par
    (spawn (vel p[3 (slew 720 0 90)]))
    (spawn (vel p[3 (evolve 0 (fn [s c]
                                (+ s (max (- 0 (* 720 (:dt c)))
                                          (min (* 720 (:dt c)) (- 90 s))))))]))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..12 {
        sim.step().unwrap();
        let a = sim.world.entities.sampled_pose(0, sim.world.tick - 1).unwrap();
        let b = sim.world.entities.sampled_pose(1, sim.world.tick - 1).unwrap();
        assert!(
            (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9,
            "slew macro vs sited evolve diverged at tick {}: ({}, {}) vs ({}, {})",
            sim.world.tick,
            a.x,
            a.y,
            b.x,
            b.y
        );
    }
}

#[test]
fn sited_evolve_counter_skips_hold_step_regions() {
    // Two sites in one component pair, the first an evolve whose init and
    // step regions are skipped once settled: the second site's index must
    // stay aligned with the static walk (the counter discipline).
    const CARD: &str = r#"
(defpattern p []
  (spawn (vel p[(evolve 1 (fn [s c] s)) (slew 720 0 90)])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    let slew_site = {
        let schema = sim.world.entities.motion_schema(0).unwrap();
        assert_eq!(schema.val_keys.len(), 2, "evolve and slew val sites");
        assert_eq!(schema.n2_keys.len(), 1, "vel state");
        schema
            .val_keys
            .iter()
            .copied()
            .find(|key| matches!(key, MotionStateKey::ScanSite { index: 1, .. }))
            .unwrap()
    };
    for _ in 0..3 {
        sim.step().unwrap();
    }
    let cell = sim.world.entities.state_val(0, slew_site).unwrap();
    let Val::Num(angle) = cell.state else { panic!("numeric slew state, got {:?}", cell.state) };
    let expected = 4.0 * 720.0 / DEFAULT_TICK_RATE;
    assert!((angle - expected).abs() < 1e-9, "slew site aligned after evolve skip: {angle} (expected {expected})");
}

#[test]
fn captured_dyn_exprs_expand_macros_at_capture() {
    // A macro expanding to a sited evolve inside a vel component: the
    // spawn-time site walk must see the expansion (the val site is
    // collected) and the state must persist — impossible if the macro
    // re-expanded per tick to a fresh construction
    // (evolve-reexpression-design.md milestone 2).
    const CARD: &str = r#"
(defmacro ramp [rate] `(evolve 0 (fn [s c] (+ s (* ~rate (:dt c))))))
(defpattern p []
  (spawn (vel c[(ramp 60) 0])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    let site = {
        let schema = sim.world.entities.motion_schema(0).unwrap();
        schema
            .val_keys
            .iter()
            .copied()
            .find(|key| matches!(key, MotionStateKey::ScanSite { .. }))
            .expect("macro-expanded evolve interns a val ScanSite slot")
    };
    for _ in 0..2 {
        sim.step().unwrap();
    }
    let cell = sim.world.entities.state_val(0, site).unwrap();
    assert_eq!(cell.tick, 3);
    let Val::Num(vx) = cell.state else { panic!("numeric state, got {:?}", cell.state) };
    let expected = 3.0 * 60.0 / DEFAULT_TICK_RATE;
    assert!((vx - expected).abs() < 1e-9, "ramped state through macro: {vx}");
}

#[test]
fn capture_time_expansion_respects_local_shadowing() {
    // A let-bound name matching a macro must be left alone by the
    // capture-time expansion (the rewrite.rs lexical discipline). The
    // macro's expansion would error if it fired.
    const CARD: &str = r#"
(defmacro speed [] `(this-name-does-not-exist))
(defpattern p []
  (spawn (vel c[(let [speed (fn [] 6)] (speed)) 0])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..2 {
        sim.step().unwrap();
    }
    let p = sim.world.entities.sampled_pose(0, sim.world.tick - 1).unwrap();
    let expected = 2.0 * 6.0 / DEFAULT_TICK_RATE;
    assert!((p.x - expected).abs() < 1e-9, "shadowed head evaluated as local fn: {}", p.x);
}

#[test]
fn standalone_evolve_step_bodies_expand_macros_at_capture() {
    // apply_evolve_step runs steps in a macro-less Ctx; a macro call in
    // the step BODY only works because the body was expanded when the
    // evolve special captured it.
    const CARD: &str = r#"
(defmacro bump [s] `(+ ~s 2))
(defpattern p []
  (spawn (evolve (cart 0 0) (fn [s c] (cart (bump (:x s)) 0)))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..3 {
        sim.step().unwrap();
    }
    // sampled at tick-1: the pose cached by the previous boundary's pass,
    // i.e. two advances after three steps
    let p = sim.world.entities.sampled_pose(0, sim.world.tick - 1).unwrap();
    assert!((p.x - 4.0).abs() < 1e-9, "macro in step body advanced: {}", p.x);
}

#[test]
fn spawned_entity_rows_carry_motion_schema() {
    const CARD: &str = r#"
(defpattern p []
  (spawn (vel p[3 (slew 720 0 90)])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    let schema = sim.world.entities.motion_schema(0).unwrap();
    assert_eq!(schema.n2_keys.len(), 1, "vel integrator state");
    assert_eq!(schema.val_keys.len(), 1, "slew evolve state");
    assert_eq!(schema.dyn_keys.len(), 0);
}

#[test]
fn vel_motion_writes_dense_state_slot() {
    const CARD: &str = r#"
(defpattern p []
  (spawn (vel c[3 0])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    let key = sim
        .world
        .entities
        .motion_schema(0)
        .unwrap()
        .n2_keys
        .iter()
        .copied()
        .find(|key| matches!(key, MotionStateKey::Node(_)))
        .unwrap();
    let [x, y] = sim.world.entities.state_n2(0, key).unwrap();
    assert!((x - (3.0 / DEFAULT_TICK_RATE)).abs() < 1e-9, "dense vel x: {x}");
    assert_eq!(y, 0.0);
}

#[test]
fn scan_sites_write_dense_state_slots() {
    const CARD: &str = r#"
(defpattern p []
  (spawn (vel p[3 (slew 720 0 90)])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    let key = sim
        .world
        .entities
        .motion_schema(0)
        .unwrap()
        .val_keys
        .iter()
        .copied()
        .find(|key| matches!(key, MotionStateKey::ScanSite { .. }))
        .unwrap();
    let cell = sim.world.entities.state_val(0, key).unwrap();
    let Val::Num(angle) = cell.state else { panic!("numeric slew state, got {:?}", cell.state) };
    assert!((angle - 6.0).abs() < 1e-9, "dense slew angle: {angle}");
}

#[test]
fn numeric_motion_reads_dense_state_before_legacy_map() {
    const CARD: &str = r#"
(defpattern p []
  (spawn (vel p[3 (slew 720 0 90)])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    let key = sim
        .world
        .entities
        .motion_schema(0)
        .unwrap()
        .val_keys
        .iter()
        .copied()
        .find(|key| matches!(key, MotionStateKey::ScanSite { .. }))
        .unwrap();
    sim.step().unwrap();
    let cell = sim.world.entities.state_val(0, key).unwrap();
    let Val::Num(angle) = cell.state else { panic!("numeric slew state, got {:?}", cell.state) };
    assert!((angle - 12.0).abs() < 1e-9, "dense slew angle after legacy clear: {angle}");
}

#[test]
fn lazy_stages_lower_to_closed_exit_cells() {
    const CARD: &str = r#"
(defpattern p []
  (spawn (stages
           (stage (ticks 1) (linear c[120 0]))
           (forever (fn [exit] (vel c[0 (* 2 (mag (:vel exit)))]))))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.step().unwrap();
    sim.step().unwrap();
    let schema = sim.world.entities.motion_schema(0).unwrap();
    assert!(schema
        .n2_keys
        .iter()
        .any(|key| matches!(key, MotionStateKey::StageExit { field: StageExitField::Pos, .. })));
    assert!(schema
        .n2_keys
        .iter()
        .any(|key| matches!(key, MotionStateKey::StageExit { field: StageExitField::Vel, .. })));
    assert!(schema.dyn_keys.is_empty());
    sim.step().unwrap();
    let p = sim.world.entities.sampled_pose(0, sim.world.tick - 1).unwrap();
    assert!((p.y - 4.0).abs() < 1e-9, "lowered stage dense y from exit velocity: {}", p.y);
}

#[test]
fn entity_motion_writes_dense_without_entity_state() {
    const CARD: &str = r#"
(defpattern p []
  (spawn (vel p[3 (slew 720 0 90)])))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    for _ in 0..4 {
        sim.step().unwrap();
    }
    let schema = sim.world.entities.motion_schema(0).unwrap();
    for key in schema.n2_keys.iter().copied() {
        assert!(sim.world.entities.state_n2(0, key).is_some());
    }
    for key in schema.val_keys.iter().copied() {
        assert!(sim.world.entities.state_val(0, key).is_some());
    }
    assert!(schema.dyn_keys.is_empty());
}

#[test]
fn entity_capacity_is_explicit() {
    const CARD: &str = r#"
(defpattern p [] (spawn (circle 2 (still))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.resize_entity_capacity(1).unwrap();
    let err = sim.step().unwrap_err();
    assert!(err.contains("entity capacity 1 exhausted"), "{err}");
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.resize_entity_capacity(2).unwrap();
    sim.step().unwrap();
    assert_eq!(live_count(&sim), 2);
}

#[test]
fn stale_handles_do_not_target_reused_rows() {
    const CARD: &str = r#"
(defchannel $enemy-hp (:hp (entities-where (matches :team :enemy))))
(defpattern p []
  (seq
    (let [old (spawn (pose c[0 0]) {:team :enemy :hp 1})]
      (seq
        (wait (ticks 1))
        (cull (first old))
        (wait (ticks 1))
        (spawn (pose c[1 0]) {:team :enemy :hp 2})
        (set-col (first old) :hp 99)
        (wait (ticks 1))))))
"#;
    let mut sim = Sim::load(CARD, Some("p")).unwrap();
    sim.resize_entity_capacity(1).unwrap();
    for _ in 0..5 {
        sim.step().unwrap();
    }
    assert_eq!(sim.world.entities.len(), 1);
    assert!(sim.world.entities.is_alive(0));
    assert_eq!(sim.world.entities.generation(0), Some(1));
    assert!(matches!(sim.ctx.sig.channel("enemy-hp"), Some(Val::Num(n)) if n == 2.0));
}

    /// Ancestor clocks are lib-expressible (§13.1 audit): a parent
    /// captures $tick into an ordinary binding (eager, an ir constant)
    /// and the child signal reads (live $tick) minus it — a
    /// pattern-epoch clock with no engine operator. The (live …) read
    /// alone must make the closed form defer (time-dependence is not
    /// just syntactic t/u), or the signal silently constant-folds at
    /// spawn to a frozen clock.
    #[test]
    fn clock_passing_is_lib() {
        const CARD: &str = r#"
(defpattern p []
  (seq
    (wait (ticks 30))
    (let [t0 $tick]
      (seq
        (spawn (cart m"(live($tick) - t0)/120" 0))
        (spawn (cart m"$tick/120" 1))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..91 {
            sim.step().unwrap();
        }
        let x = |i: usize| sim.world.entities.sampled_pos(i, sim.world.tick - 1).unwrap().0;
        // pattern-epoch clock: spawned at tick 30, sampled pos as of tick 90
        assert!((x(0) - 0.5).abs() < 0.02, "live clock minus epoch: {}", x(0));
        // without live, the channel read snaps at spawn (the boundary)
        assert!((x(1) - 0.25).abs() < 0.02, "bare read stays snapped: {}", x(1));
    }
