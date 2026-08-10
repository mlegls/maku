//! A/B behavioral fingerprint: step a card and dump every render row per
//! tick as Debug text. Diff the output of two builds to compare engines.
//! With a trailing `quiet` argument, skip the dump and print stepping wall
//! time only (for interleaved A/B timing).
//! Usage: abdump <card.maku> <pattern> <ticks> [quiet]

use maku::sim::{Inputs, Sim};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("card path");
    let pattern = args.next().expect("pattern");
    let ticks: u64 = args.next().expect("ticks").parse().expect("tick count");
    let mode = args.next();
    let quiet = mode.as_deref() == Some("quiet");
    let profile = mode.as_deref() == Some("profile");
    let src = std::fs::read_to_string(&path).expect("read card");
    let mut sim = Sim::load(&src, Some(&pattern)).expect("load");
    let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
    if quiet {
        let wall = std::time::Instant::now();
        for _ in 0..ticks {
            sim.step_with(&inputs).expect("step");
        }
        std::hint::black_box(&sim.world.render_rows);
        println!("{:.3}", wall.elapsed().as_secs_f64() * 1e3);
        return;
    }
    if profile {
        maku::interp::profile::set_enabled(true);
        for _ in 0..ticks {
            sim.step_with(&inputs).expect("step");
        }
        for (name, count, self_ns, total_ns) in maku::interp::profile::report() {
            println!(
                "{name}: self={:.2}ms total={:.2}ms n={count}",
                self_ns as f64 / 1e6,
                total_ns as f64 / 1e6
            );
        }
        return;
    }
    let mut rows = Vec::new();
    let mut out = String::new();
    for tick in 0..ticks {
        sim.step_with(&inputs).expect("step");
        rows.clear();
        for item in &sim.world.render_rows {
            item.expand_into(&mut rows);
        }
        out.push_str(&format!("tick {tick}: {} rows\n", rows.len()));
        for row in &rows {
            out.push_str(&format!("{row:?}\n"));
        }
    }
    print!("{out}");
}
