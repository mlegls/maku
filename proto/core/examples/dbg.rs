
use maku::sim::Sim;

fn main() {
    let cases: &[(&str, &str, usize)] = &[
        ("/Users/mlegls/dev/Maku/cards/translations/130_bowap.maku", "bowap", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/130_bowap.maku", "bowap-fold", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/020_gsrepeat.maku", "gsrepeat-demo", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/040_spread.maku", "spread-demo", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/060_polar.maku", "polar-demo", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/080_aimed.maku", "aimed-demo", 400),
        ("/Users/mlegls/dev/Maku/cards/translations/070_dynamic_lasers.maku", "lasers-demo", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/110_exploding_stars.maku", "exploding-stars", 400),
        ("/Users/mlegls/dev/Maku/cards/translations/200_cradle.maku", "cradle", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/player_homing.maku", "reimu-free-fire", 300),
        ("/Users/mlegls/dev/Maku/cards/translations/player_homing.maku", "reimu-focus", 400),
        ("/Users/mlegls/dev/Maku/cards/translations/player_homing.maku", "fantasy-seal", 700),
        ("/Users/mlegls/dev/Maku/cards/translations/ph_boss2_spell2.maku", "spell-2", 900),
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
