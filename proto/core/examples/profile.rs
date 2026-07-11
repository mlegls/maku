//! Flat interpreter profile over representative cards.
//!
//! Usage:
//!   cargo run --release --example profile                 # built-in set
//!   cargo run --release --example profile CARD PATTERN N  # one card
//!
//! Prints per-case and aggregate tables of (head symbol | dyn node,
//! count, self ms, inclusive ms), sorted by self time. This is the
//! instrument for the minimal-kernel add-back loop: builtins return as
//! AST-rewrite intrinsics from the top of this table.

use maku::interp::profile;
use maku::sim::Sim;
use std::collections::HashMap;

fn root(rel: &str) -> String {
    format!("{}/../../{}", env!("CARGO_MANIFEST_DIR"), rel)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cases: Vec<(String, String, usize)> = if args.len() == 3 {
        vec![(args[0].clone(), args[1].clone(), args[2].parse().expect("ticks"))]
    } else {
        [
            ("cards/translations/130_bowap.maku", "bowap", 300),
            ("cards/translations/060_polar.maku", "polar-demo", 300),
            ("cards/translations/070_dynamic_lasers.maku", "lasers-demo", 300),
            ("cards/translations/110_exploding_stars.maku", "exploding-stars", 400),
            ("cards/translations/200_cradle.maku", "cradle", 300),
            ("cards/translations/player_homing.maku", "fantasy-seal", 700),
            ("cards/translations/ph_boss2_spell2.maku", "spell-2", 900),
            ("cards/tutorials/t03.maku", "ex3-fruit-colors", 900),
            ("cards/reimu_vs_mima.maku", "reimu-vs-mima", 1800),
        ]
        .iter()
        .map(|(p, pat, n)| (root(p), pat.to_string(), *n))
        .collect()
    };

    let mut aggregate: HashMap<String, (u64, u64, u64)> = HashMap::new();
    for (path, pattern, ticks) in &cases {
        // load_file resolves the card's imports relative to its path
        let mut sim = match Sim::load_file(std::path::Path::new(path), Some(pattern)) {
            Ok(s) => s,
            Err(e) => {
                println!("LOAD FAIL {} [{}]: {}", path, pattern, e);
                continue;
            }
        };
        profile::set_enabled(true);
        let wall = std::time::Instant::now();
        let mut failed = None;
        for _ in 0..*ticks {
            if let Err(e) = sim.step() {
                failed = Some(e);
                break;
            }
        }
        let wall = wall.elapsed();
        let rows = profile::report();
        profile::set_enabled(false);

        println!("\n=== {} [{}] {} ticks, {:.1} ms wall ===", path, pattern, ticks, wall.as_secs_f64() * 1e3);
        if let Some(e) = failed {
            println!("STEP FAIL: {}", e);
            continue;
        }
        println!("{:<28} {:>10} {:>10} {:>10}", "head", "count", "self ms", "incl ms");
        for (name, count, self_ns, total_ns) in rows.iter().take(28) {
            println!(
                "{:<28} {:>10} {:>10.1} {:>10.1}",
                name,
                count,
                *self_ns as f64 / 1e6,
                *total_ns as f64 / 1e6
            );
        }
        for (name, count, self_ns, total_ns) in rows {
            let e = aggregate.entry(name).or_default();
            e.0 += count;
            e.1 += self_ns;
            e.2 += total_ns;
        }
    }

    let mut rows: Vec<_> = aggregate.into_iter().collect();
    rows.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
    println!("\n=== aggregate (all cases) ===");
    println!("{:<28} {:>10} {:>10} {:>10}", "head", "count", "self ms", "incl ms");
    for (name, (count, self_ns, total_ns)) in rows.iter().take(40) {
        println!(
            "{:<28} {:>10} {:>10.1} {:>10.1}",
            name,
            count,
            *self_ns as f64 / 1e6,
            *total_ns as f64 / 1e6
        );
    }
}
