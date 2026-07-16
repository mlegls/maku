use crate::Rgba8;

pub(crate) fn with_hue_alpha(color: Rgba8, hue_deg: f64, alpha: f64) -> [u8; 4] {
    let [r, g, b, source_alpha] = color.0;
    let (mut r, mut g, mut b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    if hue_deg.abs() >= 1e-9 {
        let (h, s, l) = rgb_to_hsl(r, g, b);
        (r, g, b) = hsl_to_rgb((h + hue_deg as f32).rem_euclid(360.0), s, l);
    }
    [byte(r), byte(g), byte(b), byte(source_alpha as f64 / 255.0 * alpha)]
}

pub(crate) fn alpha_byte(alpha: f64) -> u8 { byte(alpha) }

fn byte(value: impl Into<f64>) -> u8 {
    (value.into().clamp(0.0, 1.0) * 255.0).round() as u8
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 { return (0.0, 0.0, l); }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if (max - g).abs() < 1e-6 {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x),
        3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}
