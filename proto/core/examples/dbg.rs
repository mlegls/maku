// Census: how many scanned rows match the milestone-B batchable shape
// (ConstFrame|Translate)* -> Vel{programs: Some}, and how they group by
// program-pair pointer within one tick.

use maku::interp::DynNode;
use maku::sim::Sim;
use std::collections::HashMap;
use std::rc::Rc;

fn root_chain_vel(node: &Rc<DynNode>) -> Option<&Rc<DynNode>> {
    match &**node {
        DynNode::Vel { .. } => Some(node),
        DynNode::ConstFrame { child, .. } | DynNode::Translate { child, .. } => {
            root_chain_vel(child)
        }
        _ => None,
    }
}

fn node_name(node: &DynNode) -> &'static str {
    match node {
        DynNode::Const(_) => "const",
        DynNode::Linear { .. } => "linear",
        DynNode::ClosedPt { .. } => "closed-pt",
        DynNode::Vel { .. } => "vel",
        DynNode::Translate { .. } => "translate",
        DynNode::Path { .. } => "path",
        DynNode::Frame(..) => "frame",
        DynNode::ConstFrame { .. } => "const-frame",
        DynNode::Live { .. } | DynNode::LiveStream { .. } => "live",
        DynNode::Clamp { .. } => "clamp",
        DynNode::RotExpr { .. } => "rot-expr",
        DynNode::FnPose(_) => "fn-pose",
        DynNode::Evolve(_) => "evolve",
        DynNode::Stages { .. } => "stages",
    }
}

fn census(sim: &Sim, label: &str) {
    let mut scanned = 0usize;
    let mut alive = 0usize;
    let mut vel_match = 0usize;
    let mut vel_compiled = 0usize;
    let mut root_kinds: HashMap<String, usize> = HashMap::new();
    let mut groups: HashMap<usize, usize> = HashMap::new();
    for i in 0..sim.world.entities.len() {
        if !sim.world.entities.is_alive(i) {
            continue;
        }
        alive += 1;
        let Some(fig) = sim.world.entities.dyn_figure(i) else {
            continue;
        };
        let root = fig.pose_dyn().clone();
        *root_kinds.entry(node_name(&root).to_string()).or_default() += 1;
        if !sim.world.entities.is_scanned(i) {
            continue;
        }
        scanned += 1;
        if let Some(vel) = root_chain_vel(&root) {
            vel_match += 1;
            if let DynNode::Vel { programs, .. } = &**vel {
                if let Some(Some((ap, _))) = programs.get() {
                    vel_compiled += 1;
                    *groups.entry(Rc::as_ptr(ap) as usize).or_default() += 1;
                }
            }
        }
    }
    let mut proj_ptrs: HashMap<usize, usize> = HashMap::new();
    for i in 0..sim.world.entities.len() {
        if !sim.world.entities.is_alive(i) {
            continue;
        }
        if let Some(p) = sim.world.entities.collider_projector(i) {
            *proj_ptrs.entry(Rc::as_ptr(&p.projectors) as *const u8 as usize).or_default() += 1;
        }
    }
    println!("  distinct projector Rcs: {} over {} alive", proj_ptrs.len(), alive);
    let mut expr_kinds: HashMap<&'static str, usize> = HashMap::new();
    for i in 0..sim.world.entities.len() {
        if !sim.world.entities.is_alive(i) {
            continue;
        }
        if let Some(p) = sim.world.entities.collider_projector(i) {
            for v in p.projectors.iter() {
                use maku::interp::ColliderProjectorExpr as E;
                let (kind, radius) = match &v.expr {
                    E::Stable(s) if s.is_empty() => ("stable-empty", ""),
                    E::Stable(_) => ("stable", ""),
                    E::Circle(spec) => (
                        "circle",
                        match (&spec.radius.source, spec.radius.projection.is_some()) {
                            (maku::interp::ProjectorScalarSource::Value(_), _) => ":const",
                            (maku::interp::ProjectorScalarSource::Form(_), true) => ":field-plan",
                            (maku::interp::ProjectorScalarSource::Form(_), false) => ":expr",
                        },
                    ),
                    E::CapsuleChain(_) => ("capsule", ""),
                    E::Callable { .. } => ("callable", ""),
                    E::Cond { .. } => ("cond", ""),
                };
                *expr_kinds.entry(Box::leak(format!("{kind}{radius}").into_boxed_str())).or_default() += 1;
            }
        }
    }
    println!("  projector expr kinds: {:?}", expr_kinds);
    let mut sizes: Vec<usize> = groups.values().copied().collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    println!(
        "{label}: alive={alive} scanned={scanned} vel-chain={vel_match} compiled={vel_compiled} groups={} sizes(top12)={:?}",
        groups.len(),
        &sizes[..sizes.len().min(12)]
    );
    let mut kinds: Vec<(String, usize)> = root_kinds.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1));
    println!("  root kinds: {:?}", kinds);
}

fn main() {
    let cases: &[(&str, &str, usize)] = &[
        ("cards/tutorials/t03.maku", "ex3-fruit-colors", 900),
        ("cards/tutorials/t03.maku", "ex5-chimera", 900),
        ("cards/translations/130_bowap.maku", "bowap", 300),
        ("cards/translations/200_cradle.maku", "cradle", 300),
        ("cards/translations/player_homing.maku", "fantasy-seal", 700),
        ("cards/translations/ph_boss2_spell2.maku", "spell-2", 900),
    ];
    for (path, pattern, ticks) in cases {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                println!("READ FAIL {}: {}", path, e);
                continue;
            }
        };
        let mut sim = match Sim::load(&src, Some(pattern)) {
            Ok(s) => s,
            Err(e) => {
                println!("LOAD FAIL {} [{}]: {}", path, pattern, e);
                continue;
            }
        };
        let mut failed = false;
        for k in 0..*ticks {
            if let Err(e) = sim.step() {
                println!("STEP FAIL {} [{}] tick {}: {}", path, pattern, k, e);
                failed = true;
                break;
            }
        }
        if !failed {
            census(&sim, &format!("{path} [{pattern}] @{ticks}"));
        }
    }
}
