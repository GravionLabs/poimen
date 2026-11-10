//! The split tree of a tab drawn into the pane area: the rectangle of every leaf, the one-cell
//! borders between the children of a split, the neighbor of a leaf in a direction and the ratio
//! a resize key sets. Pure: the server's tree and a rectangle in, rectangles and ids out.

use poimen_client::protocol::{Node, SplitDirection};
use ratatui::layout::Rect;

/// A direction on screen (focus and resize keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Down,
    Up,
    Right,
}

/// Share of a split one resize key press moves.
pub const RESIZE_STEP: f64 = 0.05;
/// A resize never takes more than this off either child.
pub const MIN_RATIO: f64 = 0.1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leaf {
    pub terminal_id: String,
    pub rect: Rect,
}

/// The line between the two children of a split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Border {
    pub rect: Rect,
    pub split_id: String,
    pub direction: SplitDirection,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    pub leaves: Vec<Leaf>,
    pub borders: Vec<Border>,
}

impl Layout {
    pub fn rect_of(&self, terminal_id: &str) -> Option<Rect> {
        self.leaves
            .iter()
            .find(|l| l.terminal_id == terminal_id)
            .map(|l| l.rect)
    }

    /// The leaf under a screen position.
    pub fn leaf_at(&self, column: u16, row: u16) -> Option<&Leaf> {
        self.leaves
            .iter()
            .find(|l| l.rect.contains((column, row).into()))
    }
}

/// Lays `node` out in `area`. A split takes one cell for its border; the first child gets
/// `ratio` of the rest (at least one cell while there is room for two). A split too small for
/// a border gives everything to the first child.
pub fn layout(node: &Node, area: Rect) -> Layout {
    let mut out = Layout::default();
    place(node, area, &mut out);
    out
}

fn place(node: &Node, area: Rect, out: &mut Layout) {
    match node {
        Node::Terminal { terminal_id } => out.leaves.push(Leaf {
            terminal_id: terminal_id.clone(),
            rect: area,
        }),
        Node::Split {
            split_id,
            direction,
            ratio,
            children,
        } => {
            let total = match direction {
                SplitDirection::Right => area.width,
                SplitDirection::Down => area.height,
            };
            if total < 3 {
                place(&children[0], area, out);
                place(&children[1], Rect::new(area.x, area.y, 0, 0), out);
                return;
            }
            let room = total - 1;
            let first = (f64::from(room) * ratio)
                .round()
                .clamp(1.0, f64::from(room - 1)) as u16;
            let (a, border, b) = match direction {
                SplitDirection::Right => (
                    Rect::new(area.x, area.y, first, area.height),
                    Rect::new(area.x + first, area.y, 1, area.height),
                    Rect::new(area.x + first + 1, area.y, room - first, area.height),
                ),
                SplitDirection::Down => (
                    Rect::new(area.x, area.y, area.width, first),
                    Rect::new(area.x, area.y + first, area.width, 1),
                    Rect::new(area.x, area.y + first + 1, area.width, room - first),
                ),
            };
            out.borders.push(Border {
                rect: border,
                split_id: split_id.clone(),
                direction: *direction,
            });
            place(&children[0], a, out);
            place(&children[1], b, out);
        }
    }
}

/// How much of `a` and `b` (start, end) overlap.
fn overlap(a: (u16, u16), b: (u16, u16)) -> u16 {
    a.1.min(b.1).saturating_sub(a.0.max(b.0))
}

/// The leaf next to `from` in a direction: the nearest one that lies fully beyond its edge,
/// the one sharing the most of the edge on a tie. Leaves without a cell are never chosen.
pub fn neighbor(leaves: &[Leaf], from: &str, dir: Dir) -> Option<String> {
    let me = leaves.iter().find(|l| l.terminal_id == from)?.rect;
    leaves
        .iter()
        .filter(|l| l.terminal_id != from && l.rect.width > 0 && l.rect.height > 0)
        .filter_map(|l| {
            let r = l.rect;
            let (gap, shared) = match dir {
                Dir::Left if r.right() <= me.x => (
                    me.x - r.right(),
                    overlap((r.y, r.bottom()), (me.y, me.bottom())),
                ),
                Dir::Right if r.x >= me.right() => (
                    r.x - me.right(),
                    overlap((r.y, r.bottom()), (me.y, me.bottom())),
                ),
                Dir::Up if r.bottom() <= me.y => (
                    me.y - r.bottom(),
                    overlap((r.x, r.right()), (me.x, me.right())),
                ),
                Dir::Down if r.y >= me.bottom() => (
                    r.y - me.bottom(),
                    overlap((r.x, r.right()), (me.x, me.right())),
                ),
                _ => return None,
            };
            (shared > 0).then_some((gap, std::cmp::Reverse(shared), l.terminal_id.clone()))
        })
        .min()
        .map(|(_, _, id)| id)
}

/// The splits from the root down to the leaf `terminal_id`.
fn path<'a>(node: &'a Node, terminal_id: &str, out: &mut Vec<&'a Node>) -> bool {
    match node {
        Node::Terminal { terminal_id: id } => id == terminal_id,
        Node::Split { children, .. } => {
            out.push(node);
            if path(&children[0], terminal_id, out) || path(&children[1], terminal_id, out) {
                true
            } else {
                out.pop();
                false
            }
        }
    }
}

/// The split a resize key moves and its new ratio: the nearest split above `terminal_id` whose
/// border runs across the key's direction (a horizontal key moves the border of a `right`
/// split). `Right` and `Down` move the border towards the end, `Left` and `Up` towards the
/// start. `None` without such a split, or when the border is already at its limit.
pub fn resize(root: &Node, terminal_id: &str, dir: Dir) -> Option<(String, f64)> {
    let mut splits = Vec::new();
    if !path(root, terminal_id, &mut splits) {
        return None;
    }
    let axis = match dir {
        Dir::Left | Dir::Right => SplitDirection::Right,
        Dir::Up | Dir::Down => SplitDirection::Down,
    };
    let delta = match dir {
        Dir::Right | Dir::Down => RESIZE_STEP,
        Dir::Left | Dir::Up => -RESIZE_STEP,
    };
    splits.iter().rev().find_map(|n| match n {
        Node::Split {
            split_id,
            direction,
            ratio,
            ..
        } if *direction == axis => {
            let next = (ratio + delta).clamp(MIN_RATIO, 1.0 - MIN_RATIO);
            ((next - ratio).abs() > 1e-9).then(|| (split_id.clone(), next))
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str) -> Node {
        Node::Terminal {
            terminal_id: id.into(),
        }
    }

    fn split(id: &str, direction: SplitDirection, ratio: f64, a: Node, b: Node) -> Node {
        Node::Split {
            split_id: id.into(),
            direction,
            ratio,
            children: Box::new([a, b]),
        }
    }

    /// t1 left, t2 top right, t3 bottom right.
    fn tree() -> Node {
        split(
            "split1",
            SplitDirection::Right,
            0.5,
            leaf("t1"),
            split("split2", SplitDirection::Down, 0.5, leaf("t2"), leaf("t3")),
        )
    }

    fn rects(l: &Layout) -> Vec<(&str, Rect)> {
        l.leaves
            .iter()
            .map(|l| (l.terminal_id.as_str(), l.rect))
            .collect()
    }

    #[test]
    fn a_single_leaf_takes_the_whole_area_without_borders() {
        let l = layout(&leaf("t1"), Rect::new(3, 2, 40, 10));
        assert_eq!(rects(&l), [("t1", Rect::new(3, 2, 40, 10))]);
        assert!(l.borders.is_empty());
    }

    #[test]
    fn splits_leave_a_cell_for_the_border() {
        let l = layout(&tree(), Rect::new(0, 1, 81, 21));
        // 80 cells after the border: 40 and 40; 20 rows after the border: 10 and 10.
        assert_eq!(
            rects(&l),
            [
                ("t1", Rect::new(0, 1, 40, 21)),
                ("t2", Rect::new(41, 1, 40, 10)),
                ("t3", Rect::new(41, 12, 40, 10)),
            ]
        );
        assert_eq!(l.borders.len(), 2);
        assert_eq!(l.borders[0].rect, Rect::new(40, 1, 1, 21));
        assert_eq!(l.borders[0].split_id, "split1");
        assert_eq!(l.borders[1].rect, Rect::new(41, 11, 40, 1));
        assert_eq!(l.borders[1].direction, SplitDirection::Down);
    }

    #[test]
    fn the_ratio_sets_the_share_of_the_first_child() {
        let node = split("s", SplitDirection::Right, 0.25, leaf("a"), leaf("b"));
        let l = layout(&node, Rect::new(0, 0, 101, 5));
        assert_eq!(l.rect_of("a"), Some(Rect::new(0, 0, 25, 5)));
        assert_eq!(l.rect_of("b"), Some(Rect::new(26, 0, 75, 5)));
        // Every cell is a leaf, a border or nothing: widths add up.
        assert_eq!(25 + 1 + 75, 101);
        assert_eq!(l.rect_of("gone"), None);
    }

    #[test]
    fn extreme_ratios_and_tiny_areas_keep_every_leaf_valid() {
        let node = split("s", SplitDirection::Right, 0.999, leaf("a"), leaf("b"));
        let l = layout(&node, Rect::new(0, 0, 11, 3));
        assert_eq!(l.rect_of("a").unwrap().width, 9);
        assert_eq!(l.rect_of("b").unwrap().width, 1);
        let node = split("s", SplitDirection::Right, 0.001, leaf("a"), leaf("b"));
        assert_eq!(
            layout(&node, Rect::new(0, 0, 11, 3))
                .rect_of("a")
                .unwrap()
                .width,
            1
        );
        // No room for a border: the first child keeps the area, the second has no cells.
        let node = split("s", SplitDirection::Down, 0.5, leaf("a"), leaf("b"));
        let l = layout(&node, Rect::new(0, 0, 10, 2));
        assert_eq!(l.rect_of("a"), Some(Rect::new(0, 0, 10, 2)));
        assert_eq!(l.rect_of("b").unwrap().height, 0);
        assert!(l.borders.is_empty());
    }

    #[test]
    fn leaf_at_finds_the_pane_under_a_cell() {
        let l = layout(&tree(), Rect::new(0, 1, 81, 21));
        assert_eq!(l.leaf_at(5, 5).unwrap().terminal_id, "t1");
        assert_eq!(l.leaf_at(50, 15).unwrap().terminal_id, "t3");
        // On a border, or outside.
        assert!(l.leaf_at(40, 5).is_none());
        assert!(l.leaf_at(200, 5).is_none());
    }

    #[test]
    fn focus_moves_to_the_neighbor_in_that_direction() {
        let l = layout(&tree(), Rect::new(0, 1, 81, 21));
        let go = |from, dir| neighbor(&l.leaves, from, dir);
        assert_eq!(go("t1", Dir::Right).as_deref(), Some("t2"));
        assert_eq!(go("t2", Dir::Left).as_deref(), Some("t1"));
        assert_eq!(go("t3", Dir::Left).as_deref(), Some("t1"));
        assert_eq!(go("t2", Dir::Down).as_deref(), Some("t3"));
        assert_eq!(go("t3", Dir::Up).as_deref(), Some("t2"));
        // Nothing beyond the edge, nothing for an unknown leaf.
        assert_eq!(go("t1", Dir::Left), None);
        assert_eq!(go("t1", Dir::Up), None);
        assert_eq!(go("t3", Dir::Down), None);
        assert_eq!(go("nope", Dir::Left), None);
    }

    #[test]
    fn a_tie_goes_to_the_neighbor_sharing_more_of_the_edge() {
        // t1 spans the left; right of it t2 is small (ratio 0.2), t3 is large.
        let node = split(
            "split1",
            SplitDirection::Right,
            0.5,
            leaf("t1"),
            split("split2", SplitDirection::Down, 0.2, leaf("t2"), leaf("t3")),
        );
        let l = layout(&node, Rect::new(0, 0, 81, 21));
        assert_eq!(neighbor(&l.leaves, "t1", Dir::Right).as_deref(), Some("t3"));
    }

    #[test]
    fn resize_moves_the_nearest_split_across_the_direction() {
        let t = tree();
        // t2 sits in the `down` split inside the `right` split.
        let (id, r) = resize(&t, "t2", Dir::Down).unwrap();
        assert_eq!(id, "split2");
        assert!((r - 0.55).abs() < 1e-9);
        let (id, r) = resize(&t, "t3", Dir::Up).unwrap();
        assert_eq!(id, "split2");
        assert!((r - 0.45).abs() < 1e-9);
        // A horizontal key skips the vertical split and moves the outer one.
        let (id, r) = resize(&t, "t3", Dir::Right).unwrap();
        assert_eq!(id, "split1");
        assert!((r - 0.55).abs() < 1e-9);
        let (id, r) = resize(&t, "t1", Dir::Left).unwrap();
        assert_eq!(id, "split1");
        assert!((r - 0.45).abs() < 1e-9);
    }

    #[test]
    fn resize_has_nothing_to_move_without_a_split_or_at_the_limit() {
        assert_eq!(resize(&leaf("t1"), "t1", Dir::Right), None);
        assert_eq!(resize(&tree(), "t1", Dir::Down), None);
        assert_eq!(resize(&tree(), "nope", Dir::Right), None);
        let edge = split("s", SplitDirection::Right, 0.9, leaf("a"), leaf("b"));
        assert_eq!(resize(&edge, "a", Dir::Right), None);
        // Near the limit the step is cut to it.
        let near = split("s", SplitDirection::Right, 0.88, leaf("a"), leaf("b"));
        let (_, r) = resize(&near, "a", Dir::Right).unwrap();
        assert!((r - 0.9).abs() < 1e-9);
    }
}
