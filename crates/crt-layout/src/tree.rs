// ABOUTME: Automatic grid layout for terminal panes.
// ABOUTME: Arranges N panes in a near-square grid, adapting to window aspect ratio.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u64);

/// Rectangle in normalized coordinates (0.0 to 1.0)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn full() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug)]
pub struct LayoutTree {
    panes: Vec<PaneId>,
    focused: PaneId,
    next_id: u64,
}

impl LayoutTree {
    pub fn new() -> Self {
        let id = PaneId(0);
        Self {
            panes: vec![id],
            focused: id,
            next_id: 1,
        }
    }

    pub fn focused_pane(&self) -> PaneId {
        self.focused
    }

    pub fn set_focus(&mut self, pane: PaneId) {
        if self.panes.contains(&pane) {
            self.focused = pane;
        }
    }

    /// Hit test: given normalized coordinates (0.0-1.0), return the pane at that position
    pub fn hit_test(&self, norm_x: f32, norm_y: f32, width: f32, height: f32) -> Option<PaneId> {
        let rects = self.pane_rects(width, height);
        for (pane_id, rect) in rects {
            if norm_x >= rect.x
                && norm_x < rect.x + rect.width
                && norm_y >= rect.y
                && norm_y < rect.y + rect.height
            {
                return Some(pane_id);
            }
        }
        None
    }

    /// Add a new pane, returns its ID. New pane gets focus.
    pub fn add_pane(&mut self) -> PaneId {
        let id = PaneId(self.next_id);
        self.next_id += 1;
        self.panes.push(id);
        self.focused = id;
        id
    }

    /// Close a pane, returns the pane that should receive focus (if any remain)
    pub fn close(&mut self, pane: PaneId) -> Option<PaneId> {
        if let Some(idx) = self.panes.iter().position(|&p| p == pane) {
            self.panes.remove(idx);
            if self.panes.is_empty() {
                return None;
            }
            // Focus previous pane, or first if we removed index 0
            let new_focus_idx = if idx > 0 { idx - 1 } else { 0 };
            self.focused = self.panes[new_focus_idx];
            Some(self.focused)
        } else {
            None
        }
    }

    /// Get all pane IDs
    pub fn panes(&self) -> &[PaneId] {
        &self.panes
    }

    /// Move focus to the nearest pane in the given direction.
    /// Returns the newly focused pane, or None if no pane exists in that direction.
    pub fn focus_direction(
        &mut self,
        direction: Direction,
        width: f32,
        height: f32,
    ) -> Option<PaneId> {
        if self.panes.len() <= 1 {
            return None;
        }

        let rects = self.pane_rects(width, height);
        let focused_rect = rects.get(&self.focused)?;
        let (fx, fy) = focused_rect.center();

        let mut best: Option<(PaneId, f32)> = None;

        for (&pane_id, rect) in &rects {
            if pane_id == self.focused {
                continue;
            }

            let (cx, cy) = rect.center();
            let dx = cx - fx;
            let dy = cy - fy;

            // Check if this pane is in the right direction
            let in_direction = match direction {
                Direction::Left => dx < -0.001,
                Direction::Right => dx > 0.001,
                Direction::Up => dy < -0.001,
                Direction::Down => dy > 0.001,
            };

            if !in_direction {
                continue;
            }

            // Distance, weighted so the primary axis matters more
            let dist = match direction {
                Direction::Left | Direction::Right => dx * dx + dy * dy * 4.0,
                Direction::Up | Direction::Down => dx * dx * 4.0 + dy * dy,
            };

            if best.is_none() || dist < best.unwrap().1 {
                best = Some((pane_id, dist));
            }
        }

        if let Some((pane_id, _)) = best {
            self.focused = pane_id;
            Some(pane_id)
        } else {
            None
        }
    }

    /// Get all panes with their layout rectangles.
    /// Layout adapts to aspect ratio: landscape = columns side-by-side, portrait = rows stacked.
    pub fn pane_rects(&self, width: f32, height: f32) -> HashMap<PaneId, Rect> {
        let n = self.panes.len();
        if n == 0 {
            return HashMap::new();
        }

        let landscape = width >= height;
        let rects = compute_grid_rects(n, landscape);

        self.panes
            .iter()
            .zip(rects)
            .map(|(&id, rect)| (id, rect))
            .collect()
    }
}

/// Compute grid rectangles for N panes.
/// If landscape, major axis is horizontal (columns side by side).
/// If portrait, major axis is vertical (rows stacked).
fn compute_grid_rects(n: usize, landscape: bool) -> Vec<Rect> {
    if n == 0 {
        return vec![];
    }
    if n == 1 {
        return vec![Rect::full()];
    }

    // Number of major divisions (columns for landscape, rows for portrait)
    let major_count = (n as f32).sqrt().ceil() as usize;

    // Calculate how many items go in each major division
    // E.g., n=5, major_count=3: base=1, extra=2 → [1, 2, 2]
    let base_per_major = n / major_count;
    let extras = n % major_count;

    let mut rects = Vec::with_capacity(n);

    for major_idx in 0..major_count {
        // Extras go to the last columns/rows
        let items_in_this_major = if major_idx < major_count - extras {
            base_per_major
        } else {
            base_per_major + 1
        };

        let major_start = major_idx as f32 / major_count as f32;
        let major_size = 1.0 / major_count as f32;

        for minor_idx in 0..items_in_this_major {
            let minor_start = minor_idx as f32 / items_in_this_major as f32;
            let minor_size = 1.0 / items_in_this_major as f32;

            let rect = if landscape {
                Rect {
                    x: major_start,
                    y: minor_start,
                    width: major_size,
                    height: minor_size,
                }
            } else {
                Rect {
                    x: minor_start,
                    y: major_start,
                    width: minor_size,
                    height: major_size,
                }
            };

            rects.push(rect);
        }
    }

    rects
}

impl Default for LayoutTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.001
    }

    fn rect_approx_eq(a: &Rect, b: &Rect) -> bool {
        approx_eq(a.x, b.x)
            && approx_eq(a.y, b.y)
            && approx_eq(a.width, b.width)
            && approx_eq(a.height, b.height)
    }

    #[test]
    fn single_pane_fills_entire_space() {
        let tree = LayoutTree::new();
        let rects = tree.pane_rects(800.0, 600.0);

        assert_eq!(rects.len(), 1);
        let rect = rects.values().next().unwrap();
        assert!(rect_approx_eq(rect, &Rect::full()));
    }

    #[test]
    fn two_panes_landscape_side_by_side() {
        let mut tree = LayoutTree::new();
        tree.add_pane();

        let rects = tree.pane_rects(800.0, 600.0); // landscape
        assert_eq!(rects.len(), 2);

        // Two columns, each taking half width
        let mut sorted: Vec<_> = rects.values().collect();
        sorted.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());

        assert!(rect_approx_eq(
            sorted[0],
            &Rect {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0
            }
        ));
        assert!(rect_approx_eq(
            sorted[1],
            &Rect {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0
            }
        ));
    }

    #[test]
    fn two_panes_portrait_stacked() {
        let mut tree = LayoutTree::new();
        tree.add_pane();

        let rects = tree.pane_rects(600.0, 800.0); // portrait
        assert_eq!(rects.len(), 2);

        // Two rows, each taking half height
        let mut sorted: Vec<_> = rects.values().collect();
        sorted.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());

        assert!(rect_approx_eq(
            sorted[0],
            &Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 0.5
            }
        ));
        assert!(rect_approx_eq(
            sorted[1],
            &Rect {
                x: 0.0,
                y: 0.5,
                width: 1.0,
                height: 0.5
            }
        ));
    }

    #[test]
    fn three_panes_landscape_one_two_layout() {
        // 3 panes: 2 columns, first has 1, second has 2
        let mut tree = LayoutTree::new();
        tree.add_pane();
        tree.add_pane();

        let rects = tree.pane_rects(800.0, 600.0);
        assert_eq!(rects.len(), 3);

        let mut sorted: Vec<_> = rects.values().collect();
        sorted.sort_by(|a, b| {
            a.x.partial_cmp(&b.x)
                .unwrap()
                .then(a.y.partial_cmp(&b.y).unwrap())
        });

        // First column: full height
        assert!(rect_approx_eq(
            sorted[0],
            &Rect {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0
            }
        ));
        // Second column: two rows
        assert!(rect_approx_eq(
            sorted[1],
            &Rect {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 0.5
            }
        ));
        assert!(rect_approx_eq(
            sorted[2],
            &Rect {
                x: 0.5,
                y: 0.5,
                width: 0.5,
                height: 0.5
            }
        ));
    }

    #[test]
    fn four_panes_two_by_two_grid() {
        let mut tree = LayoutTree::new();
        tree.add_pane();
        tree.add_pane();
        tree.add_pane();

        let rects = tree.pane_rects(800.0, 600.0);
        assert_eq!(rects.len(), 4);

        // Should be 2x2 grid
        for rect in rects.values() {
            assert!(approx_eq(rect.width, 0.5));
            assert!(approx_eq(rect.height, 0.5));
        }
    }

    #[test]
    fn five_panes_one_two_two_layout() {
        // 5 panes: 3 columns with 1/2/2
        let mut tree = LayoutTree::new();
        for _ in 0..4 {
            tree.add_pane();
        }

        let rects = tree.pane_rects(800.0, 600.0);
        assert_eq!(rects.len(), 5);

        let mut sorted: Vec<_> = rects.values().collect();
        sorted.sort_by(|a, b| {
            a.x.partial_cmp(&b.x)
                .unwrap()
                .then(a.y.partial_cmp(&b.y).unwrap())
        });

        // First column: full height (1 pane)
        assert!(approx_eq(sorted[0].width, 1.0 / 3.0));
        assert!(approx_eq(sorted[0].height, 1.0));

        // Second column: 2 panes
        assert!(approx_eq(sorted[1].width, 1.0 / 3.0));
        assert!(approx_eq(sorted[1].height, 0.5));
        assert!(approx_eq(sorted[2].height, 0.5));

        // Third column: 2 panes
        assert!(approx_eq(sorted[3].width, 1.0 / 3.0));
        assert!(approx_eq(sorted[3].height, 0.5));
        assert!(approx_eq(sorted[4].height, 0.5));
    }

    #[test]
    fn six_panes_two_two_two_layout() {
        let mut tree = LayoutTree::new();
        for _ in 0..5 {
            tree.add_pane();
        }

        let rects = tree.pane_rects(800.0, 600.0);
        assert_eq!(rects.len(), 6);

        // 3 columns, 2 each
        for rect in rects.values() {
            assert!(approx_eq(rect.width, 1.0 / 3.0));
            assert!(approx_eq(rect.height, 0.5));
        }
    }

    #[test]
    fn add_pane_increases_count() {
        let mut tree = LayoutTree::new();
        assert_eq!(tree.panes().len(), 1);

        tree.add_pane();
        assert_eq!(tree.panes().len(), 2);

        tree.add_pane();
        assert_eq!(tree.panes().len(), 3);
    }

    #[test]
    fn close_pane_decreases_count() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.add_pane();

        assert_eq!(tree.panes().len(), 2);

        tree.close(second);
        assert_eq!(tree.panes().len(), 1);
        assert!(tree.panes().contains(&first));
    }

    #[test]
    fn close_updates_focus() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.add_pane();

        assert_eq!(tree.focused_pane(), second);

        tree.close(second);
        assert_eq!(tree.focused_pane(), first);
    }

    #[test]
    fn new_pane_gets_focus() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        assert_eq!(tree.focused_pane(), first);

        let second = tree.add_pane();
        assert_eq!(tree.focused_pane(), second);
    }

    #[test]
    fn hit_test_single_pane() {
        let tree = LayoutTree::new();
        let pane = tree.focused_pane();

        // Any point in the window should hit the single pane
        assert_eq!(tree.hit_test(0.0, 0.0, 800.0, 600.0), Some(pane));
        assert_eq!(tree.hit_test(0.5, 0.5, 800.0, 600.0), Some(pane));
        assert_eq!(tree.hit_test(0.99, 0.99, 800.0, 600.0), Some(pane));
    }

    #[test]
    fn hit_test_two_panes_landscape() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.add_pane();

        // In landscape (800x600), two panes are side-by-side
        // First pane: x 0.0-0.5
        // Second pane: x 0.5-1.0

        // Click in left half should hit first pane
        assert_eq!(tree.hit_test(0.25, 0.5, 800.0, 600.0), Some(first));

        // Click in right half should hit second pane
        assert_eq!(tree.hit_test(0.75, 0.5, 800.0, 600.0), Some(second));
    }

    #[test]
    fn hit_test_out_of_bounds() {
        let tree = LayoutTree::new();

        // Points outside 0.0-1.0 should return None
        assert_eq!(tree.hit_test(1.5, 0.5, 800.0, 600.0), None);
        assert_eq!(tree.hit_test(-0.1, 0.5, 800.0, 600.0), None);
    }

    #[test]
    fn focus_direction_two_panes_landscape() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.add_pane();

        // Focus is on second (right). Move left should go to first.
        assert_eq!(tree.focused_pane(), second);
        assert_eq!(
            tree.focus_direction(Direction::Left, 800.0, 600.0),
            Some(first)
        );
        assert_eq!(tree.focused_pane(), first);

        // Move right should go back to second
        assert_eq!(
            tree.focus_direction(Direction::Right, 800.0, 600.0),
            Some(second)
        );
        assert_eq!(tree.focused_pane(), second);
    }

    #[test]
    fn focus_direction_two_panes_portrait() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.add_pane();

        // Portrait: panes are stacked vertically
        assert_eq!(
            tree.focus_direction(Direction::Up, 600.0, 800.0),
            Some(first)
        );
        assert_eq!(tree.focused_pane(), first);

        assert_eq!(
            tree.focus_direction(Direction::Down, 600.0, 800.0),
            Some(second)
        );
        assert_eq!(tree.focused_pane(), second);
    }

    #[test]
    fn focus_direction_no_pane_in_direction() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        tree.add_pane();
        tree.set_focus(first);

        // In landscape, first pane is leftmost — can't go further left
        assert_eq!(tree.focus_direction(Direction::Left, 800.0, 600.0), None);
        assert_eq!(tree.focused_pane(), first);
    }

    #[test]
    fn focus_direction_single_pane() {
        let mut tree = LayoutTree::new();
        assert_eq!(tree.focus_direction(Direction::Right, 800.0, 600.0), None);
    }

    #[test]
    fn focus_direction_four_pane_grid() {
        // 4 panes in landscape (800x600) = 2x2 grid:
        // [first]  [third]
        // [second] [fourth]
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.add_pane();
        let third = tree.add_pane();
        let fourth = tree.add_pane();

        // Start at first (top-left)
        tree.set_focus(first);

        // Right should go to third (top-right)
        assert_eq!(
            tree.focus_direction(Direction::Right, 800.0, 600.0),
            Some(third)
        );

        // Down from third should go to fourth (bottom-right)
        assert_eq!(
            tree.focus_direction(Direction::Down, 800.0, 600.0),
            Some(fourth)
        );

        // Left from fourth should go to second (bottom-left)
        assert_eq!(
            tree.focus_direction(Direction::Left, 800.0, 600.0),
            Some(second)
        );

        // Up from second should go to first (top-left)
        assert_eq!(
            tree.focus_direction(Direction::Up, 800.0, 600.0),
            Some(first)
        );
    }
}
