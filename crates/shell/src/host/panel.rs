//! Shared panel geometry in logical output coordinates.
use cabby_config::{BarPosition, Config};
use iced_exwlshell::reexport::{Anchor, LayerSize};
#[derive(Debug, Clone, Copy)]
pub struct Origin {
    pub output: u32,
    pub x: f32,
}
pub struct Geometry {
    pub width: u32,
    pub height: u32,
    pub anchor: Anchor,
    pub margin: (i32, i32, i32, i32),
    pub shield_anchor: Anchor,
    pub shield_size: LayerSize,
}
pub fn geometry(config: &Config, size: (u32, u32), x: f32) -> Geometry {
    let gap = 8u32.min(size.0 / 2).min(size.1 / 2);
    let width = size.0.saturating_sub(gap * 2).clamp(1, 440);
    let clearance = u32::from(config.bar_height).min(size.1.saturating_sub(1));
    let height = size.1.saturating_sub(clearance + gap * 2).clamp(1, 500);
    let left = (x - width as f32 / 2.0)
        .max(gap as f32)
        .min(size.0.saturating_sub(width + gap) as f32)
        .max(0.0) as i32;
    let edge = clearance
        .saturating_add(gap)
        .min(size.1.saturating_sub(height)) as i32;
    let top = config.bar_position == BarPosition::Top;
    Geometry {
        width,
        height,
        anchor: Anchor::Left | if top { Anchor::Top } else { Anchor::Bottom },
        margin: if top {
            (edge, 0, 0, left)
        } else {
            (0, 0, edge, left)
        },
        shield_anchor: Anchor::Left
            | Anchor::Right
            | if top { Anchor::Bottom } else { Anchor::Top },
        shield_size: LayerSize::fill_width(size.1.saturating_sub(clearance).max(1)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn follows_bar_edge_and_bounds_each_output_independently() {
        for position in [BarPosition::Top, BarPosition::Bottom] {
            let config = Config {
                bar_position: position,
                ..Config::default()
            };
            for size in [(1920, 1080), (320, 400), (80, 50), (1, 1)] {
                for x in [-100.0, 0.0, 160.0, 4000.0] {
                    let g = geometry(&config, size, x);
                    assert!(g.width <= size.0 && g.height <= size.1);
                    assert!(g.margin.3 >= 0 && g.margin.3 as u32 + g.width <= size.0);
                    let vertical = if position == BarPosition::Top {
                        g.margin.0
                    } else {
                        g.margin.2
                    };
                    assert!(vertical as u32 + g.height <= size.1);
                    assert!(g.anchor.contains(if position == BarPosition::Top {
                        Anchor::Top
                    } else {
                        Anchor::Bottom
                    }));
                }
            }
        }
    }
}
