//! A ROG mouse seen from the top, with each bindable button's choice at its
//! side and a leader line to the button (#435) — the way GearLink shows it,
//! drawn in our own style (no ASUS artwork). Only the buttons every ROG
//! mouse has in the same place are drawn: left, right, the wheel (up, click,
//! down) and the two thumb buttons on the left side. The DPI button differs
//! per model and is shown below the drawing by the caller.

use crate::theme::DialogColors;

use eframe::egui::{self, pos2, vec2, CornerRadius, Pos2, Rect, Stroke, StrokeKind};

/// The action codes of the buttons drawn here (`MouseProtocol.Buttons`).
pub const LEFT: u16 = 0x01F0;
pub const RIGHT: u16 = 0x01F1;
pub const WHEEL_CLICK: u16 = 0x01F2;
pub const BACK: u16 = 0x01E4;
pub const FORWARD: u16 = 0x01E5;
pub const WHEEL_UP: u16 = 0x01E8;
pub const WHEEL_DOWN: u16 = 0x01E9;

/// Which side a button's choice sits on, top to bottom.
const LEFT_COLUMN: [u16; 3] = [LEFT, FORWARD, BACK];
const RIGHT_COLUMN: [u16; 4] = [RIGHT, WHEEL_UP, WHEEL_CLICK, WHEEL_DOWN];

const CALLOUT_W: f32 = 176.0;
const CALLOUT_H: f32 = 48.0;
const BODY: [f32; 2] = [124.0, 216.0];
/// The body's front and back corner radii.
const BODY_FRONT_RADIUS: u8 = 58;
const BODY_BACK_RADIUS: u8 = 48;
const HEIGHT: f32 = 4.0 * CALLOUT_H + 36.0;

/// Whether the drawing shows `source`'s button (the others go in rows).
pub fn drawn(source: u16) -> bool {
    LEFT_COLUMN.contains(&source) || RIGHT_COLUMN.contains(&source)
}

/// Draws the mouse and, for each drawn button, `choice(ui, source, width)`
/// at its side — the caller's dropdown. A choice hovered (or its popup
/// open) highlights its button.
pub fn draw(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    names: &dyn Fn(u16) -> String,
    choice: &mut dyn FnMut(&mut egui::Ui, u16, f32) -> egui::Response,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), egui::Sense::hover());
    let body = Rect::from_center_size(
        pos2(rect.center().x, rect.top() + 18.0 + BODY[1] / 2.0),
        vec2(BODY[0], BODY[1]),
    );
    let geo = Geometry::new(body);

    // The choices first, so the drawing knows which one is hovered.
    let mut hovered: Option<u16> = None;
    let mut anchors: Vec<(u16, Pos2)> = Vec::new();
    let columns: [(&[u16], f32, bool); 2] = [
        (&LEFT_COLUMN, rect.left() + 4.0, true),
        (&RIGHT_COLUMN, rect.right() - 4.0 - CALLOUT_W, false),
    ];
    for (column, x, left_side) in columns {
        for (i, &source) in column.iter().enumerate() {
            // The left column lines up with its buttons: left at the top,
            // the thumb buttons level with the side.
            let row = if left_side {
                [0.0, 2.0, 3.0][i]
            } else {
                i as f32
            };
            let top = rect.top() + 8.0 + row * CALLOUT_H;
            let area = Rect::from_min_size(pos2(x, top), vec2(CALLOUT_W, CALLOUT_H));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(
                egui::Layout::top_down(if left_side {
                    egui::Align::Min
                } else {
                    egui::Align::Max
                }),
            ));
            child.label(
                egui::RichText::new(names(source))
                    .size(11.0)
                    .color(dc.muted),
            );
            let resp = choice(&mut child, source, CALLOUT_W);
            if resp.hovered() || resp.has_focus() || egui::Popup::is_id_open(ui.ctx(), resp.id) {
                hovered = Some(source);
            }
            let y = resp.rect.center().y;
            anchors.push((
                source,
                pos2(
                    if left_side {
                        resp.rect.right() + 6.0
                    } else {
                        resp.rect.left() - 6.0
                    },
                    y,
                ),
            ));
        }
    }

    let p = ui.painter();
    let line = Stroke::new(1.0_f32, dc.card_border);
    let lit = dc.tab_active;

    // The body: rounded, wider at the back, a seam between the two main buttons.
    p.rect(
        body,
        CornerRadius {
            nw: BODY_FRONT_RADIUS,
            ne: BODY_FRONT_RADIUS,
            sw: BODY_BACK_RADIUS,
            se: BODY_BACK_RADIUS,
        },
        dc.inset,
        Stroke::new(1.5_f32, dc.label),
        StrokeKind::Inside,
    );
    let fill = lit.gamma_multiply(0.45);
    match hovered {
        // A main button lights in its own shape: the rounded front, the
        // outer edge and the seams around the wheel.
        Some(source @ (LEFT | RIGHT)) => {
            for piece in geo.main_button(source == LEFT) {
                p.add(egui::Shape::convex_polygon(piece, fill, Stroke::NONE));
            }
        }
        Some(source) => {
            if let Some((_, region)) = geo.regions().into_iter().find(|(s, _)| *s == source) {
                p.rect_filled(region, CornerRadius::same(4), fill);
            }
        }
        None => {}
    }
    p.line_segment(
        [geo.seam_top, geo.seam_bottom],
        Stroke::new(1.0_f32, dc.label),
    );
    p.line_segment(
        [geo.buttons_left, geo.buttons_right],
        Stroke::new(1.0_f32, dc.label),
    );
    p.rect(
        geo.wheel,
        CornerRadius::same(6),
        dc.card,
        Stroke::new(1.5_f32, dc.label),
        StrokeKind::Inside,
    );
    for thumb in [geo.forward, geo.back] {
        p.rect(
            thumb,
            CornerRadius::same(3),
            dc.card,
            Stroke::new(1.5_f32, dc.label),
            StrokeKind::Inside,
        );
    }

    // Leader lines: from each choice to its button, an elbow at the body's edge.
    for (source, from) in anchors {
        let to = geo.target(source);
        let stroke = if hovered == Some(source) {
            Stroke::new(1.5_f32, lit)
        } else {
            line
        };
        let elbow = pos2(
            if from.x < to.x {
                body.left() - 14.0
            } else {
                body.right() + 14.0
            },
            from.y,
        );
        p.line_segment([from, elbow], stroke);
        p.line_segment([elbow, to], stroke);
        p.circle_filled(
            to,
            2.5,
            if hovered == Some(source) {
                lit
            } else {
                dc.label
            },
        );
    }
}

/// Where each part of the mouse sits inside `body`.
struct Geometry {
    seam_top: Pos2,
    seam_bottom: Pos2,
    buttons_left: Pos2,
    buttons_right: Pos2,
    wheel: Rect,
    forward: Rect,
    back: Rect,
    body: Rect,
}

impl Geometry {
    fn new(body: Rect) -> Self {
        let cx = body.center().x;
        let split = body.top() + body.height() * 0.42;
        Self {
            seam_top: pos2(cx, body.top()),
            seam_bottom: pos2(cx, split),
            buttons_left: pos2(body.left() + 6.0, split),
            buttons_right: pos2(body.right() - 6.0, split),
            wheel: Rect::from_center_size(
                pos2(cx, body.top() + body.height() * 0.2),
                vec2(16.0, 40.0),
            ),
            forward: Rect::from_min_size(pos2(body.left() - 3.0, split + 14.0), vec2(8.0, 26.0)),
            back: Rect::from_min_size(pos2(body.left() - 3.0, split + 44.0), vec2(8.0, 26.0)),
            body,
        }
    }

    /// The point a leader line ends at.
    fn target(&self, source: u16) -> Pos2 {
        // The main buttons' points sit near the front, above the wheel's, so
        // every leader line keeps its callout's top-to-bottom order and none
        // crosses another.
        let quarter = self.body.width() / 4.0;
        let main_y = self.body.top() + self.body.height() * 0.1;
        match source {
            LEFT => pos2(self.body.left() + quarter * 1.1, main_y),
            RIGHT => pos2(self.body.right() - quarter * 1.1, main_y),
            WHEEL_UP => self.wheel.center_top() + vec2(0.0, 5.0),
            WHEEL_CLICK => self.wheel.center(),
            WHEEL_DOWN => self.wheel.center_bottom() - vec2(0.0, 5.0),
            FORWARD => self.forward.center(),
            BACK => self.back.center(),
            _ => self.body.center(),
        }
    }

    /// A main button's surface as two convex pieces (`left` or right): the
    /// rounded front down to just below the wheel, beside the wheel, then the
    /// part below it down to the seam — each inset from the outline.
    fn main_button(&self, left: bool) -> [Vec<Pos2>; 2] {
        let body = self.body.shrink(2.5);
        let cx = self.body.center().x;
        let radius = f32::from(BODY_FRONT_RADIUS) - 2.5;
        let inner = self.wheel.left() - 3.0; // the left button's edge by the wheel
        let below_wheel = self.wheel.bottom() + 3.0;
        let split = self.seam_bottom.y - 2.5;
        // Built for the left button, mirrored for the right.
        let centre = pos2(body.left() + radius, body.top() + radius);
        // The front is nearly a half circle: its arc starts where it meets the
        // wheel's edge, not at its top.
        let start = ((inner - centre.x) / radius).clamp(-1.0, 1.0).acos();
        let end = std::f32::consts::PI;
        let mut front: Vec<Pos2> = (0..=16)
            .map(|k| {
                let a = start + (end - start) * k as f32 / 16.0;
                pos2(centre.x + a.cos() * radius, centre.y - a.sin() * radius)
            })
            .collect();
        front.push(pos2(body.left(), below_wheel));
        front.push(pos2(inner, below_wheel));
        let rest = vec![
            pos2(inner, below_wheel),
            pos2(body.left(), below_wheel),
            pos2(body.left(), split),
            pos2(cx - 1.5, split),
            pos2(cx - 1.5, below_wheel),
        ];
        let mirror = |points: Vec<Pos2>| -> Vec<Pos2> {
            if left {
                points
            } else {
                // Mirrored, the winding flips — reverse to keep it.
                points
                    .into_iter()
                    .rev()
                    .map(|p| pos2(2.0 * cx - p.x, p.y))
                    .collect()
            }
        };
        [mirror(front), mirror(rest)]
    }

    /// The area lit while a button's choice is hovered.
    fn regions(&self) -> [(u16, Rect); 7] {
        let cx = self.body.center().x;
        let main = |left: bool| {
            Rect::from_min_max(
                pos2(
                    if left {
                        self.body.left() + 8.0
                    } else {
                        cx + 3.0
                    },
                    self.body.top() + 10.0,
                ),
                pos2(
                    if left {
                        cx - 3.0
                    } else {
                        self.body.right() - 8.0
                    },
                    self.seam_bottom.y - 3.0,
                ),
            )
        };
        let wheel_part = |from: f32, to: f32| {
            Rect::from_min_max(
                pos2(
                    self.wheel.left() - 3.0,
                    self.wheel.top() + self.wheel.height() * from,
                ),
                pos2(
                    self.wheel.right() + 3.0,
                    self.wheel.top() + self.wheel.height() * to,
                ),
            )
        };
        [
            (LEFT, main(true)),
            (RIGHT, main(false)),
            (WHEEL_UP, wheel_part(-0.1, 0.33)),
            (WHEEL_CLICK, wheel_part(0.33, 0.67)),
            (WHEEL_DOWN, wheel_part(0.67, 1.1)),
            (FORWARD, self.forward.expand(3.0)),
            (BACK, self.back.expand(3.0)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_main_buttons_light_as_convex_shapes_inside_the_body_beside_the_wheel() {
        let body = Rect::from_center_size(pos2(200.0, 150.0), vec2(BODY[0], BODY[1]));
        let geo = Geometry::new(body);
        for left in [true, false] {
            for piece in geo.main_button(left) {
                assert!(piece.len() >= 3);
                assert!(
                    piece.iter().all(|p| body.expand(0.5).contains(*p)),
                    "{piece:?}"
                );
                // Every turn the same way: convex.
                let turns: Vec<f32> = (0..piece.len())
                    .map(|i| {
                        let (a, b, c) = (
                            piece[i],
                            piece[(i + 1) % piece.len()],
                            piece[(i + 2) % piece.len()],
                        );
                        (b - a).x * (c - b).y - (b - a).y * (c - b).x
                    })
                    .filter(|t| t.abs() > 1e-3)
                    .collect();
                assert!(
                    turns.iter().all(|t| *t > 0.0) || turns.iter().all(|t| *t < 0.0),
                    "{piece:?}"
                );
                // Never over the wheel.
                assert!(piece.iter().all(|p| p.x <= geo.wheel.left() - 2.0
                    || p.x >= geo.wheel.right() + 2.0
                    || p.y >= geo.wheel.bottom()));
            }
        }
    }

    #[test]
    fn every_button_but_the_dpi_button_is_drawn() {
        for source in [
            LEFT,
            RIGHT,
            WHEEL_CLICK,
            BACK,
            FORWARD,
            WHEEL_UP,
            WHEEL_DOWN,
        ] {
            assert!(drawn(source), "{source:04X}");
        }
        assert!(!drawn(0x01E6));
    }

    #[test]
    fn leader_lines_end_inside_the_body() {
        let body = Rect::from_center_size(pos2(200.0, 150.0), vec2(BODY[0], BODY[1]));
        let geo = Geometry::new(body);
        for source in [LEFT, RIGHT, WHEEL_CLICK, WHEEL_UP, WHEEL_DOWN] {
            assert!(body.contains(geo.target(source)), "{source:04X}");
        }
        // The thumb buttons sit on the left edge.
        assert!(geo.target(FORWARD).x < body.left() + 4.0);
        assert!(geo.target(BACK).y > geo.target(FORWARD).y);
        // Each column's points run top to bottom like its callouts, so no
        // leader line crosses another.
        let ys = |c: &[u16]| c.iter().map(|s| geo.target(*s).y).collect::<Vec<_>>();
        for column in [LEFT_COLUMN.as_slice(), RIGHT_COLUMN.as_slice()] {
            let y = ys(column);
            assert!(y.windows(2).all(|w| w[0] < w[1]), "{y:?}");
        }
    }
}
