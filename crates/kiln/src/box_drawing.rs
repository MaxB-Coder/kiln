//! Box-drawing and block characters drawn as rectangles that fill the cell exactly, so lines
//! join without seams at any size (font glyphs rarely line up perfectly). Characters not
//! covered here (rounded corners, dashes, diagonals) still come from the font.

/// How heavy one arm of a box-drawing character is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    None,
    Light,
    Heavy,
    Double,
}

/// A rectangle in cell units (0..1 on both axes), plus its opacity (shades).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub alpha: f32,
}

fn piece(x: f32, y: f32, w: f32, h: f32) -> Piece {
    Piece {
        x,
        y,
        w,
        h,
        alpha: 1.,
    }
}

/// Arms (up, right, down, left) of the box-drawing characters handled here.
fn arms(c: char) -> Option<[Arm; 4]> {
    use Arm::{Double as D, Heavy as H, Light as L, None as N};
    Some(match c {
        '─' => [N, L, N, L],
        '━' => [N, H, N, H],
        '│' => [L, N, L, N],
        '┃' => [H, N, H, N],
        '┌' => [N, L, L, N],
        '┏' => [N, H, H, N],
        '┐' => [N, N, L, L],
        '┓' => [N, N, H, H],
        '└' => [L, L, N, N],
        '┗' => [H, H, N, N],
        '┘' => [L, N, N, L],
        '┛' => [H, N, N, H],
        '├' => [L, L, L, N],
        '┣' => [H, H, H, N],
        '┤' => [L, N, L, L],
        '┫' => [H, N, H, H],
        '┬' => [N, L, L, L],
        '┳' => [N, H, H, H],
        '┴' => [L, L, N, L],
        '┻' => [H, H, N, H],
        '┼' => [L, L, L, L],
        '╋' => [H, H, H, H],
        '╴' => [N, N, N, L],
        '╵' => [L, N, N, N],
        '╶' => [N, L, N, N],
        '╷' => [N, N, L, N],
        '═' => [N, D, N, D],
        '║' => [D, N, D, N],
        '╔' => [N, D, D, N],
        '╗' => [N, N, D, D],
        '╚' => [D, D, N, N],
        '╝' => [D, N, N, D],
        '╠' => [D, D, D, N],
        '╣' => [D, N, D, D],
        '╦' => [N, D, D, D],
        '╩' => [D, D, N, D],
        '╬' => [D, D, D, D],
        _ => return None,
    })
}

/// The pieces that draw `c`, given the line thickness in cell units for each axis
/// (`tx` for vertical strokes, `ty` for horizontal ones). `None` means "use the font".
pub fn pieces(c: char, tx: f32, ty: f32) -> Option<Vec<Piece>> {
    if let Some(arms) = arms(c) {
        return Some(box_pieces(arms, tx, ty));
    }
    let eighth = 1. / 8.;
    let quad = |tl: bool, tr: bool, bl: bool, br: bool| {
        let mut v = Vec::new();
        if tl {
            v.push(piece(0., 0., 0.5, 0.5));
        }
        if tr {
            v.push(piece(0.5, 0., 0.5, 0.5));
        }
        if bl {
            v.push(piece(0., 0.5, 0.5, 0.5));
        }
        if br {
            v.push(piece(0.5, 0.5, 0.5, 0.5));
        }
        v
    };
    let shade = |alpha: f32| {
        vec![Piece {
            x: 0.,
            y: 0.,
            w: 1.,
            h: 1.,
            alpha,
        }]
    };
    Some(match c {
        '█' => vec![piece(0., 0., 1., 1.)],
        '▀' => vec![piece(0., 0., 1., 0.5)],
        '▔' => vec![piece(0., 0., 1., eighth)],
        '▕' => vec![piece(1. - eighth, 0., eighth, 1.)],
        '▐' => vec![piece(0.5, 0., 0.5, 1.)],
        // Lower eighths ▁ to ▇ (▄, the half, is in the middle) and left eighths ▉ to ▏.
        '▁'..='▇' => {
            let n = (c as u32 - '▁' as u32 + 1) as f32;
            vec![piece(0., 1. - n * eighth, 1., n * eighth)]
        }
        '▉'..='▏' => {
            // ▉ is 7/8 wide, down to ▏ at 1/8 (▌, the half, is in the middle).
            let n = (7 - (c as u32 - '▉' as u32)) as f32;
            vec![piece(0., 0., n * eighth, 1.)]
        }
        '░' => shade(0.25),
        '▒' => shade(0.5),
        '▓' => shade(0.75),
        '▖' => quad(false, false, true, false),
        '▗' => quad(false, false, false, true),
        '▘' => quad(true, false, false, false),
        '▝' => quad(false, true, false, false),
        '▙' => quad(true, false, true, true),
        '▚' => quad(true, false, false, true),
        '▛' => quad(true, true, true, false),
        '▜' => quad(true, true, false, true),
        '▞' => quad(false, true, true, false),
        '▟' => quad(false, true, true, true),
        _ => return None,
    })
}

fn box_pieces([up, right, down, left]: [Arm; 4], tx: f32, ty: f32) -> Vec<Piece> {
    let mut out = Vec::new();
    // Stroke widths per weight; double lines are two light strokes with a gap.
    let width = |arm: Arm, t: f32| match arm {
        Arm::Heavy => t * 2.,
        _ => t,
    };
    let cx = 0.5;
    let cy = 0.5;
    // Half-length of the centre crossing, so arms meet the widest perpendicular stroke.
    let reach_x = [up, down]
        .iter()
        .map(|a| match a {
            Arm::None => 0.,
            Arm::Double => tx * 1.5,
            a => width(*a, tx) / 2.,
        })
        .fold(0., f32::max);
    let reach_y = [left, right]
        .iter()
        .map(|a| match a {
            Arm::None => 0.,
            Arm::Double => ty * 1.5,
            a => width(*a, ty) / 2.,
        })
        .fold(0., f32::max);

    // Horizontal arms.
    for (arm, from, to) in [(left, 0., cx + reach_x), (right, cx - reach_x, 1.)] {
        match arm {
            Arm::None => {}
            Arm::Double => {
                for offset in [-ty, ty] {
                    out.push(piece(from, cy + offset - ty / 2., to - from, ty));
                }
            }
            _ => {
                let w = width(arm, ty);
                out.push(piece(from, cy - w / 2., to - from, w));
            }
        }
    }
    // Vertical arms.
    for (arm, from, to) in [(up, 0., cy + reach_y), (down, cy - reach_y, 1.)] {
        match arm {
            Arm::None => {}
            Arm::Double => {
                for offset in [-tx, tx] {
                    out.push(piece(cx + offset - tx / 2., from, tx, to - from));
                }
            }
            _ => {
                let w = width(arm, tx);
                out.push(piece(cx - w / 2., from, w, to - from));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_line_spans_the_cell() {
        let p = pieces('─', 0.1, 0.05).unwrap();
        let total: f32 = p.iter().map(|p| p.w).sum();
        assert!(p.iter().all(|p| p.h > 0.04 && p.h < 0.06));
        assert!(total >= 1.0);
    }

    #[test]
    fn eighths_and_shades() {
        assert_eq!(pieces('▁', 0.1, 0.1).unwrap()[0].h, 1. / 8.);
        assert_eq!(pieces('▇', 0.1, 0.1).unwrap()[0].h, 7. / 8.);
        assert_eq!(pieces('▉', 0.1, 0.1).unwrap()[0].w, 7. / 8.);
        assert_eq!(pieces('▏', 0.1, 0.1).unwrap()[0].w, 1. / 8.);
        assert_eq!(pieces('▒', 0.1, 0.1).unwrap()[0].alpha, 0.5);
    }

    #[test]
    fn rounded_corners_use_the_font() {
        assert!(pieces('╭', 0.1, 0.1).is_none());
        assert!(pieces('a', 0.1, 0.1).is_none());
    }
}
