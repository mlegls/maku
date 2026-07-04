
use danmaku_core::sim::Sim;

fn main() {
    let cases: &[(&str, &str, usize)] = &[
        ("/Users/mlegls/dev/danmaku-engine/translations/130_bowap.edn", "bowap", 300),
        ("/Users/mlegls/dev/danmaku-engine/translations/130_bowap.edn", "bowap-fold", 300),
        ("/Users/mlegls/dev/danmaku-engine/translations/020_gsrepeat.edn", "gsrepeat-demo", 300),
        ("/Users/mlegls/dev/danmaku-engine/translations/040_spread.edn", "spread-demo", 300),
        ("/Users/mlegls/dev/danmaku-engine/translations/060_polar.edn", "polar-demo", 300),
        ("/Users/mlegls/dev/danmaku-engine/translations/080_aimed.edn", "aimed-demo", 400),
        ("/Users/mlegls/dev/danmaku-engine/translations/070_dynamic_lasers.edn", "lasers-demo", 300),
        ("/Users/mlegls/dev/danmaku-engine/translations/110_exploding_stars.edn", "exploding-stars", 400),
        ("/Users/mlegls/dev/danmaku-engine/translations/200_cradle.edn", "cradle", 300),
    ];
    for (path, pattern, ticks) in cases {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => { println!("READ FAIL {}: {}", path, e); continue; }
        };
        let mut sim = match Sim::load(&src, Some(pattern)) {
            Ok(s) => s,
            Err(e) => { println!("LOAD FAIL {} [{}]: {}", path, pattern, e); continue; }
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
            println!("OK   {} [{}]: {} bullets", path, pattern, sim.world.bullets.len());
        }
    }
}
