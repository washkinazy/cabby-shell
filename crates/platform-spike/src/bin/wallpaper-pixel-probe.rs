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
        mode if mode.starts_with("session-menu-") => {
            let n: Vec<u32> = mode
                .trim_start_matches("session-menu-")
                .split('-')
                .map(|v| v.parse().unwrap())
                .collect();
            let (lw, lh, x, y, width, height) = (n[0], n[1], n[2], n[3], n[4], n[5]);
            let px = |v: u32| v * w / lw;
            let py = |v: u32| v * h / lh;
            check(px(x + width / 2), py(y + 8), [24, 27, 34]);
            let mut ink = 0;
            for yy in py(y + 12)..py(y + height - 12) {
                for xx in px(x + 12)..px(x + width - 12) {
                    let i = ((yy * w + xx) * 4) as usize;
                    ink += usize::from(
                        asset.rgba[i..i + 3]
                            .iter()
                            .zip([229u8, 233, 240])
                            .all(|(a, b)| a.abs_diff(b) < 12),
                    );
                }
            }
            assert!(ink > 150, "session menu content missing: {ink}");
        }
        "bar-all-icons" => {
            for index in 0..9 {
                let right = w - 8 - index * 34;
                let mut ink = 0;
                for y in 10..30 {
                    for x in right - 26..right {
                        let i = ((y * w + x) * 4) as usize;
                        ink += usize::from(asset.rgba[i..i + 3].iter().all(|v| *v > 100));
                    }
                }
                assert!(ink > 8, "missing bar icon {index}: {ink}");
            }
        }
        mode if mode.starts_with("bar-hint-") => {
            let n: Vec<u32> = mode
                .trim_start_matches("bar-hint-")
                .split('-')
                .map(|v| v.parse().unwrap())
                .collect();
            let (lw, lh, x, y, width, height, light) =
                (n[0], n[1], n[2], n[3], n[4], n[5], n[6] != 0);
            let px = |v: u32| v * w / lw;
            let py = |v: u32| v * h / lh;
            check(
                px(x + width / 2),
                py(y + 4),
                if light { [231, 231, 236] } else { [35, 38, 45] },
            );
            let foreground = if light {
                [32u8, 33, 40]
            } else {
                [229u8, 233, 240]
            };
            let mut ink = 0;
            for yy in py(y + 8)..py(y + height - 8) {
                for xx in px(x + 8)..px(x + width - 8) {
                    let i = ((yy * w + xx) * 4) as usize;
                    ink += usize::from(
                        asset.rgba[i..i + 3]
                            .iter()
                            .zip(foreground)
                            .all(|(a, b)| a.abs_diff(b) < 8),
                    );
                }
            }
            assert!(ink > 15, "hint text has no readable contrast: {ink}");
        }
        mode if mode.starts_with("osd-meter-") => {
            let values: Vec<i32> = mode
                .trim_start_matches("osd-meter-")
                .split('-')
                .map(|s| s.parse().unwrap())
                .collect();
            // The sentinel for a lock-key card is encoded as 101 (no meter).
            let (percent, lw, lh, margin) = (
                values[0],
                values[1] as u32,
                values[2] as u32,
                values[3] as u32,
            );
            let pw = 320.min(lw.saturating_sub(16).max(1));
            let ph = 100.min(lh);
            let (left, top) = ((lw - pw) / 2, lh - margin.min(lh - ph) - ph);
            let sx = |x: u32| (u64::from(x) * u64::from(w) / u64::from(lw)) as u32;
            let sy = |y: u32| (u64::from(y) * u64::from(h) / u64::from(lh)) as u32;
            check(sx(left + 8), sy(top + 8), [24, 27, 34]);
            let mut longest = 0;
            let mut bright = 0;
            for y in sy(top + 16)..sy(top + ph - 12) {
                let mut run = 0;
                for x in sx(left + 16)..sx(left + pw - 16) {
                    let i = ((y * w + x) * 4) as usize;
                    let rgb = &asset.rgba[i..i + 3];
                    if rgb
                        .iter()
                        .zip([136u8, 192, 208])
                        .all(|(a, b)| a.abs_diff(b) <= 3)
                    {
                        run += 1;
                        longest = longest.max(run);
                    } else {
                        run = 0;
                    }
                    bright += usize::from(rgb.iter().all(|v| *v > 180));
                }
            }
            let expected = if percent == 101 {
                0
            } else {
                ((pw - 32) as f32 * w as f32 / lw as f32 * percent as f32 / 100.).round() as i32
            };
            assert!(
                (longest - expected).abs() <= 4 && bright > 20,
                "OSD meter/text missing: {longest}, expected {expected}, text {bright}"
            );
        }
        mode if mode.starts_with("fluent-bar-") => {
            let rows = if mode.ends_with("top") {
                0..40
            } else {
                h - 40..h
            };
            let mut accent = 0;
            let mut selected = 0;
            for y in rows {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    let rgb = &asset.rgba[i..i + 3];
                    accent += usize::from(
                        rgb.iter()
                            .zip([136u8, 192, 208])
                            .all(|(a, b)| a.abs_diff(b) <= 2),
                    );
                    selected += usize::from(
                        rgb.iter()
                            .zip([49u8, 52, 59])
                            .all(|(a, b)| a.abs_diff(b) <= 2),
                    );
                }
            }
            assert!(
                (16..300).contains(&accent) && selected > 100,
                "workspace marker/neutral selection missing: {accent}/{selected}"
            );
        }
        mode if mode.starts_with("shot-select-ready-") => {
            let dims: Vec<u32> = mode
                .trim_start_matches("shot-select-ready-")
                .split('-')
                .map(|v| v.parse().unwrap())
                .collect();
            let (lw, lh) = (dims[0], dims[1]);
            for x in [lw - 61, lw - 164] {
                let px = (u64::from(x) * u64::from(w) / u64::from(lw)) as u32;
                let py = (u64::from(lh - 22) * u64::from(h) / u64::from(lh)) as u32;
                let i = ((py * w + px) * 4) as usize;
                let rgb = &asset.rgba[i..i + 3];
                assert!(
                    rgb.iter()
                        .zip([35u8, 38, 45])
                        .all(|(a, b)| a.abs_diff(b) <= 2),
                    "selector controls not ready"
                );
            }
        }
        mode if mode.starts_with("shot-ready-") => {
            let dims: Vec<u32> = mode
                .trim_start_matches("shot-ready-")
                .split('-')
                .map(|v| v.parse().unwrap())
                .collect();
            let (lw, lh) = (dims[0], dims[1]);
            let (pw, ph) = (
                440.min(lw.saturating_sub(16)),
                500.min(lh.saturating_sub(56)),
            );
            let (left, top) = ((lw - pw) / 2, (lh - ph) / 2);
            for (button, x) in [left + 24, left + pw / 2, left + pw - 24]
                .into_iter()
                .enumerate()
            {
                let px = (u64::from(x) * u64::from(w) / u64::from(lw)) as u32;
                let py = (u64::from(top + ph - 22) * u64::from(h) / u64::from(lh)) as u32;
                let index = ((py * w + px) * 4) as usize;
                let rgb = &asset.rgba[index..index + 3];
                assert!(
                    if button == 1 {
                        rgb[0] >= 120
                            && rgb[1] >= 170
                            && rgb[2] >= 180
                            && rgb[2].saturating_sub(rgb[0]) > 50
                    } else {
                        rgb.iter()
                            .zip([35u8, 38, 45])
                            .all(|(a, b)| a.abs_diff(b) <= 2)
                    },
                    "preview controls not ready"
                );
            }
        }
        "capture-background" => {
            for (x, y) in [(w / 2, h / 2), (100.min(w - 1), 100.min(h - 1))] {
                check(x, y, [25, 51, 77]);
            }
        }
        "capture-solid" => {
            for (x, y) in [(0, 0), (w / 2, h / 2), (w - 1, h - 1)] {
                check(x, y, [25, 51, 77]);
            }
        }
        mode if mode.starts_with("caffeine-") => {
            let start = if mode.ends_with("top") { 0 } else { h - 40 };
            let mut icon = 0;
            let mut active = 0;
            for y in start + 4..start + 36 {
                for x in 8..40 {
                    let offset = ((y * w + x) * 4) as usize;
                    let rgb = &asset.rgba[offset..offset + 3];
                    if if mode.contains("-on-") {
                        rgb.iter().all(|a| *a < 90)
                    } else {
                        rgb.iter().all(|a| *a > 180)
                    } {
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
            let mut fill = 0;
            let mut icon = 0;
            let mut outside = 0;
            for y in start + 8..start + 32 {
                for x in 8..110 {
                    let offset = ((y * w + x) * 4) as usize;
                    let rgb = &asset.rgba[offset..offset + 3];
                    if rgb
                        .iter()
                        .zip([24u8, 27, 34])
                        .all(|(a, b)| a.abs_diff(b) <= 2)
                    {
                        fill += usize::from(x < 34);
                    }
                    if rgb.iter().all(|a| *a > 180) {
                        icon += usize::from(x < 34);
                        outside += usize::from(x >= 40);
                    }
                }
            }
            assert!(
                fill > 250 && icon > 10 && outside == 0,
                "compact idle/night-light icon missing or prose remains: {fill}/{icon}/{outside}"
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
            // Shared Fluent panels use the configured opaque surface, including
            // their otherwise empty padding, over the synthetic wallpaper.
            check(
                left_x + (10.0 * scale) as u32,
                ((top + 10.0) * scale) as u32,
                [24, 27, 34],
            );
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
