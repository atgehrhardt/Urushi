//! Terminal panes: geometry (pane rectangles, grid sizes, dividers, cell hit-testing)
//! and painting a screen into a scene.

use urushi_client::{ScreenState, v1};
use urushi_core::Axis;
use urushi_core::layout::{self, Layout};
use urushi_ui::{CellMetrics, Color, Font, Point, Quad, Rect, Scene, TerminalPalette};

use crate::state::Selection;

/// Space between a pane's edge and its text.
pub const PANE_PADDING: f32 = 8.0;
/// Visual gap between panes (the divider sits in it).
pub const DIVIDER: f32 = 1.0;
/// How far from a divider a press still grabs it.
pub const DIVIDER_GRAB: f32 = 4.0;

fn to_ui(r: layout::Rect) -> Rect {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "layout geometry is in screen-sized f64 units"
    )]
    Rect::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32)
}

fn to_layout(r: Rect) -> layout::Rect {
    layout::Rect {
        x: f64::from(r.x),
        y: f64::from(r.y),
        width: f64::from(r.width),
        height: f64::from(r.height),
    }
}

/// Each pane's rectangle inside `area`, with room for dividers.
pub fn pane_rects(layout: &Layout, area: Rect) -> Vec<(String, Rect)> {
    let geometry = layout.geometry(to_layout(area));
    geometry
        .panes
        .iter()
        .map(|p| {
            let r = to_ui(p.rect);
            // Shave half a divider off each inner edge.
            let left = if r.x > area.x + 0.5 {
                DIVIDER / 2.0
            } else {
                0.0
            };
            let top = if r.y > area.y + 0.5 {
                DIVIDER / 2.0
            } else {
                0.0
            };
            let right = if r.right() < area.right() - 0.5 {
                DIVIDER / 2.0
            } else {
                0.0
            };
            let bottom = if r.bottom() < area.bottom() - 0.5 {
                DIVIDER / 2.0
            } else {
                0.0
            };
            (
                p.pane.to_string(),
                Rect::new(
                    r.x + left,
                    r.y + top,
                    r.width - left - right,
                    r.height - top - bottom,
                ),
            )
        })
        .collect()
}

/// A divider and the line it is drawn on.
#[derive(Clone, Debug, PartialEq)]
pub struct DividerHit {
    /// The divider's anchor pane.
    pub anchor: String,
    /// Its split's axis.
    pub axis: Axis,
    /// The split's rectangle.
    pub split: Rect,
    /// The line (zero width or height).
    pub line: Rect,
}

/// Every divider in the layout.
pub fn dividers(layout: &Layout, area: Rect) -> Vec<DividerHit> {
    layout
        .geometry(to_layout(area))
        .dividers
        .iter()
        .map(|d| {
            let split = to_ui(d.split);
            #[allow(clippy::cast_possible_truncation, reason = "a 0–1 fraction")]
            let at = d.position as f32;
            let line = match d.axis {
                Axis::Row => Rect::new(
                    split.x + split.width * at - DIVIDER / 2.0,
                    split.y,
                    DIVIDER,
                    split.height,
                ),
                Axis::Column => Rect::new(
                    split.x,
                    split.y + split.height * at - DIVIDER / 2.0,
                    split.width,
                    DIVIDER,
                ),
            };
            DividerHit {
                anchor: d.anchor.to_string(),
                axis: d.axis,
                split,
                line,
            }
        })
        .collect()
}

/// The divider under `p`, if any.
pub fn divider_at(layout: &Layout, area: Rect, p: Point) -> Option<DividerHit> {
    dividers(layout, area).into_iter().find(|d| {
        let grab = Rect::new(
            d.line.x - DIVIDER_GRAB,
            d.line.y - DIVIDER_GRAB,
            d.line.width + 2.0 * DIVIDER_GRAB,
            d.line.height + 2.0 * DIVIDER_GRAB,
        );
        grab.contains(p)
    })
}

/// The divider position (fraction of its split) for a drag to `p`.
pub fn divider_position(axis: Axis, split: Rect, p: Point) -> f64 {
    let fraction = match axis {
        Axis::Row => (p.x - split.x) / split.width.max(1.0),
        Axis::Column => (p.y - split.y) / split.height.max(1.0),
    };
    f64::from(fraction.clamp(0.0, 1.0))
}

/// Columns and rows that fit in a pane.
pub fn grid_size(pane: Rect, cell: CellMetrics) -> (u16, u16) {
    let fit = |space: f32, unit: f32| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 1..=1000"
        )]
        let n = ((space - 2.0 * PANE_PADDING) / unit.max(1.0))
            .floor()
            .clamp(1.0, 1000.0) as u16;
        n
    };
    (fit(pane.width, cell.width), fit(pane.height, cell.height))
}

/// The cell under `p`, clamped to the grid.
pub fn cell_at(pane: Rect, cell: CellMetrics, screen: &ScreenState, p: Point) -> (u16, u16) {
    let clamp = |v: f32, max: u16| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to the grid"
        )]
        let n = v.floor().clamp(0.0, f32::from(max.saturating_sub(1))) as u16;
        n
    };
    let col = clamp((p.x - pane.x - PANE_PADDING) / cell.width, screen.cols);
    let row = clamp((p.y - pane.y - PANE_PADDING) / cell.height, screen.rows);
    (row, col)
}

/// The selected text, rows joined with newlines and trailing blanks trimmed.
pub fn selection_text(screen: &ScreenState, selection: &Selection) -> String {
    let ((r0, c0), (r1, c1)) = selection.ordered();
    let mut out = Vec::new();
    for row in r0..=r1 {
        let cells = screen.cells(usize::from(row));
        let from = if row == r0 { usize::from(c0) } else { 0 };
        let to = if row == r1 {
            usize::from(c1) + 1
        } else {
            usize::MAX
        };
        let line: String = cells
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= from && *i < to)
            .map(|(_, c)| c.text.as_str())
            .collect();
        out.push(line.trim_end().to_owned());
    }
    out.join("\n")
}

/// How to paint one pane.
pub struct PaneView<'a> {
    /// The screen.
    pub screen: &'a ScreenState,
    /// Colours.
    pub palette: &'a TerminalPalette,
    /// Cell size.
    pub cell: CellMetrics,
    /// Font size.
    pub font_size: f32,
    /// Has keyboard focus.
    pub focused: bool,
    /// Text selection, if any.
    pub selection: Option<Selection>,
}

fn resolve(color: Option<&v1::Color>, palette: &TerminalPalette, default: Color) -> Color {
    match color.and_then(|c| c.kind) {
        Some(v1::color::Kind::Indexed(i)) => palette.indexed(u8::try_from(i).unwrap_or(7)),
        Some(v1::color::Kind::Rgb(rgb)) => Color::hex(rgb),
        None => default,
    }
}

/// Paints a pane: backgrounds, selection, cursor, then text.
pub fn paint_pane(scene: &mut Scene, rect: Rect, view: &PaneView<'_>) {
    let p = view.palette;
    let cell = view.cell;
    scene.rect(rect, p.background);
    scene.push_clip(rect);
    let origin = Point::new(rect.x + PANE_PADDING, rect.y + PANE_PADDING);
    let mut glyphs: Vec<(Point, String, Font, Color)> = Vec::new();
    for row in 0..view.screen.rows {
        let y = origin.y + f32::from(row) * cell.height;
        for (col, c) in view.screen.cells(usize::from(row)).iter().enumerate() {
            #[allow(clippy::cast_precision_loss, reason = "column counts are small")]
            let x = origin.x + col as f32 * cell.width;
            let style = c.style.unwrap_or_default();
            let flags = style.flags;
            let mut fg = resolve(style.fg.as_ref(), p, p.foreground);
            let mut bg = resolve(style.bg.as_ref(), p, p.background);
            if flags & v1::StyleFlag::Inverse as u32 != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags & v1::StyleFlag::Dim as u32 != 0 {
                fg = fg.mix(bg, 0.4);
            }
            let width = cell.width * f32::from(c.width.max(1));
            let cell_rect = Rect::new(x, y, width, cell.height);
            if bg != p.background {
                scene.rect(cell_rect, bg);
            }
            #[allow(clippy::cast_possible_truncation, reason = "column counts fit in u16")]
            if view.selection.is_some_and(|s| s.contains(row, col as u16)) {
                scene.rect(cell_rect, p.selection);
            }
            if flags & v1::StyleFlag::Underline as u32 != 0 {
                scene.rect(Rect::new(x, y + cell.height - 2.0, width, 1.0), fg);
            }
            if flags & v1::StyleFlag::Strikethrough as u32 != 0 {
                scene.rect(Rect::new(x, y + cell.height / 2.0, width, 1.0), fg);
            }
            if !c.text.is_empty() && c.text != " " && flags & v1::StyleFlag::Hidden as u32 == 0 {
                let font = Font::Mono {
                    bold: flags & v1::StyleFlag::Bold as u32 != 0,
                    italic: flags & v1::StyleFlag::Italic as u32 != 0,
                };
                glyphs.push((Point::new(x, y), c.text.clone(), font, fg));
            }
        }
    }
    paint_cursor(scene, origin, view, &mut glyphs);
    for (at, text, font, color) in glyphs {
        scene.text(at, text, font, view.font_size, color);
    }
    scene.pop_clip();
}

fn paint_cursor(
    scene: &mut Scene,
    origin: Point,
    view: &PaneView<'_>,
    glyphs: &mut [(Point, String, Font, Color)],
) {
    let cursor = view.screen.cursor;
    if !cursor.visible || view.screen.display_offset > 0 {
        return;
    }
    let cell = view.cell;
    #[allow(clippy::cast_precision_loss, reason = "cursor coordinates are small")]
    let at = Point::new(
        origin.x + cursor.col as f32 * cell.width,
        origin.y + cursor.line as f32 * cell.height,
    );
    let color = view.palette.cursor;
    let shape = v1::CursorShape::try_from(cursor.shape).unwrap_or(v1::CursorShape::Block);
    match (view.focused, shape) {
        (true, v1::CursorShape::Bar) => scene.rect(Rect::new(at.x, at.y, 2.0, cell.height), color),
        (true, v1::CursorShape::Underline) => scene.rect(
            Rect::new(at.x, at.y + cell.height - 2.0, cell.width, 2.0),
            color,
        ),
        (true, _) => {
            scene.rect(Rect::new(at.x, at.y, cell.width, cell.height), color);
            // The character under a block cursor is drawn in the background colour.
            for glyph in glyphs
                .iter_mut()
                .filter(|g| (g.0.x - at.x).abs() < 0.5 && (g.0.y - at.y).abs() < 0.5)
            {
                glyph.3 = view.palette.background;
            }
        }
        (false, _) => scene.quad(Quad {
            rect: Rect::new(at.x, at.y, cell.width, cell.height),
            color: Color::TRANSPARENT,
            radius: 0.0,
            border_width: 1.0,
            border_color: color.with_alpha(0.7),
        }),
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "exact positions are expected")]
mod tests {
    use super::*;
    use urushi_core::{Direction, PaneId};

    fn p(n: u128) -> PaneId {
        PaneId::from_u128(n)
    }

    const CELL: CellMetrics = CellMetrics {
        width: 8.0,
        height: 16.0,
    };

    #[test]
    fn panes_leave_room_for_dividers_and_report_grid_sizes() {
        let mut layout = Layout::single(p(1));
        layout.split(p(1), p(2), Direction::Right).unwrap();
        let area = Rect::new(0.0, 0.0, 801.0, 400.0);
        let rects = pane_rects(&layout, area);
        assert_eq!(rects.len(), 2);
        assert!((rects[0].1.right() + DIVIDER - rects[1].1.x).abs() < 0.01);
        let (cols, rows) = grid_size(rects[0].1, CELL);
        assert_eq!((cols, rows), (48, 24));
    }

    #[test]
    fn dividers_are_grabbable_and_positions_clamp() {
        let mut layout = Layout::single(p(1));
        layout.split(p(1), p(2), Direction::Down).unwrap();
        let area = Rect::new(0.0, 0.0, 400.0, 400.0);
        let hit = divider_at(&layout, area, Point::new(100.0, 202.0)).unwrap();
        assert_eq!(hit.axis, Axis::Column);
        assert!(divider_at(&layout, area, Point::new(100.0, 100.0)).is_none());
        assert!(
            (divider_position(Axis::Column, hit.split, Point::new(0.0, 100.0)) - 0.25).abs() < 1e-6
        );
        assert_eq!(
            divider_position(Axis::Column, hit.split, Point::new(0.0, -50.0)),
            0.0
        );
    }

    #[test]
    fn selection_text_spans_lines() {
        let mut screen = ScreenState {
            cols: 10,
            rows: 2,
            ..ScreenState::default()
        };
        let span = |t: &str| v1::Span {
            text: t.into(),
            style: None,
        };
        screen.lines = vec![vec![span("hello")], vec![span("world  ")]];
        let s = Selection {
            anchor: (1, 2),
            head: (0, 1),
        };
        assert_eq!(selection_text(&screen, &s), "ello\nwor");
        let pane = Rect::new(0.0, 0.0, 200.0, 100.0);
        assert_eq!(
            cell_at(
                pane,
                CELL,
                &screen,
                Point::new(PANE_PADDING + 17.0, PANE_PADDING + 20.0)
            ),
            (1, 2)
        );
        assert_eq!(
            cell_at(pane, CELL, &screen, Point::new(-10.0, 999.0)),
            (1, 0)
        );
    }

    #[test]
    fn painting_draws_text_cursor_and_selection() {
        let palette = urushi_ui::Theme::dark().terminal;
        let mut screen = ScreenState {
            cols: 10,
            rows: 2,
            ..ScreenState::default()
        };
        screen.lines = vec![
            vec![v1::Span {
                text: "$ ls".into(),
                style: None,
            }],
            vec![],
        ];
        screen.cursor = v1::Cursor {
            line: 0,
            col: 4,
            visible: true,
            shape: v1::CursorShape::Block.into(),
        };
        let mut scene = Scene::new();
        let view = PaneView {
            screen: &screen,
            palette: &palette,
            cell: CELL,
            font_size: 13.0,
            focused: true,
            selection: Some(Selection {
                anchor: (0, 0),
                head: (0, 1),
            }),
        };
        paint_pane(&mut scene, Rect::new(0.0, 0.0, 200.0, 60.0), &view);
        let dump = scene.dump();
        assert!(dump.contains("\"$\" mono"));
        assert!(dump.contains("\"l\" mono"));
        assert!(dump.contains(&palette.selection.to_hex()));
        assert!(dump.contains(&palette.cursor.to_hex()));
    }
}
