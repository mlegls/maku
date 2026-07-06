
use maku_core::sim::Sim;

fn main() {
    let cases: &[(&str, &str, usize)] = &[
        ("/Users/mlegls/dev/Maku/cards/translations/130_bowap.dmk", "bowap", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/130_bowap.dmk", "bowap-fold", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/020_gsrepeat.dmk", "gsrepeat-demo", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/040_spread.dmk", "spread-demo", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/060_polar.dmk", "polar-demo", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/080_aimed.dmk", "aimed-demo", 400),
        ("/Users/mlegls/dev/Maku/cards/translations/070_dynamic_lasers.dmk", "lasers-demo", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/110_exploding_stars.dmk", "exploding-stars", 400),
        ("/Users/mlegls/dev/Maku/cards/translations/200_cradle.dmk", "cradle", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/player_homing.dmk", "reimu-free-fire", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/player_homing.dmk", "reimu-focus", 400),
        ("/Users/mlegls/dev/Maku/cards/translations/player_homing.dmk", "fantasy-seal", 700),
        ("/Users/mlegls/dev/Maku/cards/translations/ph_boss2_spell2.dmk", "spell-2", 900),
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
