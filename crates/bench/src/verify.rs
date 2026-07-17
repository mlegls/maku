use crate::contract::{RuleClass, Workload, WorkloadFamily};
use maku::{render::RenderItem, sim::Sim};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticObservation {
    pub live_entities: usize,
    pub render_lanes: usize,
    pub render_lanes_by_kind: BTreeMap<String, usize>,
    pub contacts_per_tick: u64,
    pub rule_matches_per_tick: u64,
    pub rule_actions_per_tick: u64,
    pub state_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verification {
    pub valid: bool,
    pub observed: SemanticObservation,
    pub errors: Vec<String>,
}

pub fn observe(sim: &mut Sim, workload: &Workload) -> SemanticObservation {
    let counters = sim.benchmark_counters();
    let live_entities = counters.live_entities;
    let mut render_lanes = 0;
    let mut render_lanes_by_kind = BTreeMap::new();
    for item in sim.render_frame() {
        let (kind, lanes) = match item {
            RenderItem::Row(row) => (row.kind.to_string(), 1),
            RenderItem::Batch(batch) => (batch.kind.to_string(), batch.len),
        };
        render_lanes += lanes;
        *render_lanes_by_kind.entry(kind).or_default() += lanes;
    }
    let rule_actions_per_tick = match workload.rules.class {
        RuleClass::MaskedUpdate => counters.rule_actions as u64,
        RuleClass::EffectAction => counters.rule_actions as u64,
        RuleClass::RenderOnly => render_lanes as u64,
        RuleClass::FilterOnly | RuleClass::None => 0,
    };
    SemanticObservation {
        live_entities,
        render_lanes,
        render_lanes_by_kind,
        contacts_per_tick: counters.contacts as u64,
        rule_matches_per_tick: counters.predicate_matches as u64,
        rule_actions_per_tick,
        state_digest: format!("{:016x}", sim.benchmark_digest()),
    }
}

pub fn verify(sim: &mut Sim, workload: &Workload) -> Verification {
    let observed = observe(sim, workload);
    let expected = &workload.expect;
    let mut errors = Vec::new();
    if observed.live_entities != expected.live_entities { errors.push(format!("live entities: expected {}, got {}", expected.live_entities, observed.live_entities)); }
    if observed.render_lanes != expected.render_lanes { errors.push(format!("render lanes: expected {}, got {}", expected.render_lanes, observed.render_lanes)); }
    if observed.contacts_per_tick != expected.contacts_per_tick { errors.push(format!("contacts/tick: expected {}, got {}", expected.contacts_per_tick, observed.contacts_per_tick)); }
    if workload.family == WorkloadFamily::Rules && observed.rule_matches_per_tick != expected.rule_matches_per_tick { errors.push(format!("rule matches/tick: expected {}, got {}", expected.rule_matches_per_tick, observed.rule_matches_per_tick)); }
    if workload.family == WorkloadFamily::Rules && observed.rule_actions_per_tick != expected.rule_actions_per_tick { errors.push(format!("rule actions/tick: expected {}, got {}", expected.rule_actions_per_tick, observed.rule_actions_per_tick)); }
    Verification { valid: errors.is_empty(), observed, errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generate, Workload};

    #[test]
    fn digest_and_semantics_repeat() {
        let workload: Workload = serde_json::from_str(include_str!("../../../bench/workloads/v1/bullets-continuity.json")).unwrap();
        let generated = generate(&workload).unwrap();
        let run = || {
            let mut sim = Sim::load(&generated.source, Some("bench")).unwrap();
            sim.resize_entity_capacity(workload.entities.capacity.unwrap()).unwrap();
            sim.step().unwrap();
            verify(&mut sim, &workload)
        };
        let a = run();
        let b = run();
        assert!(a.valid, "{:?}", a.errors);
        assert_eq!(a, b);
    }
}
