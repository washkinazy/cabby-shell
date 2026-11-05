//! Compare synthetic nested-session captures; never modifies a display.
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "wallpaper-pixel-probe CAPTURE MODE");
    let config = cabby_config::Config {
        wallpaper_image: args[1].clone(),
        ..Default::default()
    };
    let asset = cabby_config::wallpaper::load(&config).unwrap().unwrap();
    let (w, h) = (asset.width, asset.height);
    let check = |x: u32, y: u32, expected: [u8; 3]| {
        let offset = ((y * w + x) * 4) as usize;
        let actual = &asset.rgba[offset..offset + 3];
        assert!(
            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
            "pixel ({x},{y}): {actual:?}, expected {expected:?}"
        );
    };
    let background = [25, 51, 77];
    let left = [40, 90, 130];
    let right = [180, 130, 60];
    match args[2].as_str() {
        mode if mode.starts_with("notifications-") => {
            let scale = if mode.contains("scaled") { 1.5f32 } else { 1.0 };
            let top = if mode.ends_with("top") { 48.0 } else { 8.0 };
            let bottom = if mode.ends_with("bottom") { 40.0 } else { 0.0 };
            let card_height = ((h as f32 / scale - top - 24.0 - bottom) / 3.0)
                .clamp(1.0, 180.0)
                .floor();
            let x = w - (24.0 * scale) as u32;
            let pixel = |y: f32| {
                let offset = (((y * scale) as u32 * w + x) * 4) as usize;
                [
                    asset.rgba[offset],
                    asset.rgba[offset + 1],
                    asset.rgba[offset + 2],
                ]
            };
            let card = pixel(top + card_height - 16.0);
            assert_ne!(card, background, "first card missing");
            for slot in 1..3 {
                check(
                    x,
                    ((top + slot as f32 * (card_height + 8.0) + card_height - 16.0) * scale) as u32,
                    card,
                );
                check(
                    x,
                    ((top + slot as f32 * (card_height + 8.0) - 4.0) * scale) as u32,
                    background,
                );
            }
        }
        "solid" => {
            check(w / 4, h / 2, background);
            check(w * 3 / 4, h - 80, background);
        }
        "fit" => {
            check(w / 4, h / 2, left);
            check(w * 3 / 4, h / 2, right);
            check(w / 2, h - 80, background);
        }
        "fill" | "stretch" => {
            check(w / 4, h - 80, left);
            check(w * 3 / 4, h - 80, right);
        }
        "center" => {
            check(w / 2 - 4, h / 2, left);
            check(w / 2 + 4, h / 2, right);
            check(w / 4, h / 2, background);
        }
        _ => panic!("unknown probe mode"),
    }
    println!("wallpaper {} pixels passed", args[2]);
}
