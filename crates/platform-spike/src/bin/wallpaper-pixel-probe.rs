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
        mode if mode.starts_with("caffeine-") => {
            let start = if mode.ends_with("top") { 0 } else { h - 40 };
            let mut icon = 0;
            let mut active = 0;
            for y in start + 4..start + 36 {
                for x in 8..40 {
                    let offset = ((y * w + x) * 4) as usize;
                    let rgb = &asset.rgba[offset..offset + 3];
                    if rgb.iter().all(|a| *a > 180) {
                        icon += 1;
                    }
                    if rgb[0] > 100 && rgb[1] > 80 && rgb[2] > 80 {
                        active += 1;
                    }
                }
            }
            assert!(icon > 20, "caffeine vector missing: {icon}");
            if mode.contains("-on-") {
                assert!(active > 150, "caffeine active state missing: {active}");
            }
        }
        mode if mode.starts_with("idle-") => {
            let start = if mode.ends_with("top") { 0 } else { h - 40 };
            let mut accent = 0;
            let mut letters = 0;
            for y in start + 8..start + 32 {
                for x in 16..100 {
                    let offset = ((y * w + x) * 4) as usize;
                    let rgb = &asset.rgba[offset..offset + 3];
                    if rgb
                        .iter()
                        .zip([136u8, 192, 208])
                        .all(|(a, b)| a.abs_diff(b) < 8)
                    {
                        accent += 1;
                    }
                    if rgb.iter().all(|a| *a < 90) {
                        letters += 1;
                    }
                }
            }
            assert!(
                accent > 500 && letters > 60,
                "idle button/text missing: {accent}/{letters}"
            );
        }
        mode if mode.starts_with("clock-") => {
            let rows = if mode.ends_with("top") {
                0..40
            } else {
                h - 40..h
            };
            let mut xs = Vec::new();
            for y in rows {
                for x in 0..w {
                    let offset = ((y * w + x) * 4) as usize;
                    if asset.rgba[offset..offset + 3]
                        .iter()
                        .zip([229u8, 233, 240])
                        .all(|(a, b)| a.abs_diff(b) < 50)
                    {
                        xs.push(x);
                    }
                }
            }
            assert!(xs.len() > 100, "clock text missing: {} pixels", xs.len());
            let left = *xs.iter().min().unwrap();
            let right = *xs.iter().max().unwrap();
            if mode.contains("-left-") {
                assert!(left < 16 && right < w / 3, "left bounds: {left}..{right}");
            } else if mode.contains("-right-") {
                assert!(
                    right > w - 16 && left > w * 2 / 3,
                    "right bounds: {left}..{right}"
                );
            } else {
                assert!(
                    ((left + right) / 2).abs_diff(w / 2) < 4,
                    "middle bounds: {left}..{right}, width {w}"
                );
            }
        }
        mode if mode.starts_with("audio-")
            || mode.starts_with("tray-")
            || mode.starts_with("clipboard-") =>
        {
            // The nested audio probe runs after scale 1.5 is established.
            let scale = if mode.starts_with("clipboard-") {
                1.0f32
            } else {
                1.5f32
            };
            let top = if mode.ends_with("-top") {
                48.0
            } else {
                (h as f32 / scale - 548.0).max(8.0)
            };
            let left_x = w.saturating_sub((448.0 * scale) as u32);
            let mut text_pixels = 0;
            for y in ((top + 12.0) * scale) as u32..((top + 300.0) * scale).min(h as f32) as u32 {
                for x in left_x + 20..w.saturating_sub(24) {
                    let offset = ((y * w + x) * 4) as usize;
                    if asset.rgba[offset..offset + 3]
                        .iter()
                        .zip([229u8, 233, 240])
                        .all(|(a, b)| a.abs_diff(b) < 20)
                    {
                        text_pixels += 1;
                    }
                }
            }
            if mode.starts_with("tray-") {
                let rows = if mode.ends_with("-top") {
                    0..60
                } else {
                    h - 60..h
                };
                let icon_pixels = rows
                    .flat_map(|y| (0..w).map(move |x| (y * w + x) as usize * 4))
                    .filter(|offset| {
                        asset.rgba[*offset..*offset + 3]
                            .iter()
                            .zip([40u8, 50, 60])
                            .all(|(a, b)| a.abs_diff(b) < 3)
                    })
                    .count();
                assert!(icon_pixels > 4, "tray icon missing: {icon_pixels} pixels");
            }
            assert!(
                text_pixels
                    > if mode.starts_with("clipboard-") {
                        200
                    } else {
                        300
                    },
                "panel content missing: {text_pixels} text pixels"
            );
        }
        "launcher-icon" => {
            let pixels = asset
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| {
                    p[..3]
                        .iter()
                        .zip([237u8, 53, 155])
                        .all(|(a, b)| a.abs_diff(b) < 5)
                })
                .count();
            assert!(pixels > 100, "launcher icon missing");
        }
        "bar-top" => check(w / 2, 1, [24, 27, 34]),
        "bar-bottom" => check(w / 2, h - 2, [24, 27, 34]),
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
