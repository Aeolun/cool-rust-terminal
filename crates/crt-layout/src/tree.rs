// ABOUTME: Binary tree structure for terminal pane layout.
// ABOUTME: Supports splitting, closing, resizing, and focus navigation.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Debug)]
enum Node {
    Pane(PaneId),
    Split {
        direction: Direction,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Debug)]
pub struct LayoutTree {
    root: Node,
    focused: PaneId,
    next_id: u64,
}

/// Rectangle in normalized coordinates (0.0 to 1.0)
#[derive(Debug, Clone, Copy)]
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
}

impl LayoutTree {
    pub fn new() -> Self {
        let id = PaneId(0);
        Self {
            root: Node::Pane(id),
            focused: id,
            next_id: 1,
        }
    }

    pub fn focused_pane(&self) -> PaneId {
        self.focused
    }

    pub fn set_focus(&mut self, pane: PaneId) {
        self.focused = pane;
    }

    /// Get all panes with their layout rectangles
    pub fn pane_rects(&self) -> HashMap<PaneId, Rect> {
        let mut result = HashMap::new();
        collect_rects(&self.root, Rect::full(), &mut result);
        result
    }

    /// Split the given pane, returns the new pane's ID
    pub fn split(&mut self, pane: PaneId, direction: Direction) -> Option<PaneId> {
        let new_id = PaneId(self.next_id);
        self.next_id += 1;

        if split_node(&mut self.root, pane, direction, new_id) {
            self.focused = new_id;
            Some(new_id)
        } else {
            None
        }
    }

    /// Close a pane, returns the pane that should receive focus (if any remain)
    pub fn close(&mut self, pane: PaneId) -> Option<PaneId> {
        if let Some(sibling_id) = close_node(&mut self.root, pane) {
            self.focused = sibling_id;
            Some(sibling_id)
        } else {
            None
        }
    }

    /// Get all pane IDs
    pub fn panes(&self) -> Vec<PaneId> {
        let mut result = Vec::new();
        collect_panes(&self.root, &mut result);
        result
    }
}

fn collect_rects(node: &Node, rect: Rect, out: &mut HashMap<PaneId, Rect>) {
    match node {
        Node::Pane(id) => {
            out.insert(*id, rect);
        }
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let (first_rect, second_rect) = match direction {
                Direction::Horizontal => (
                    Rect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width * ratio,
                        height: rect.height,
                    },
                    Rect {
                        x: rect.x + rect.width * ratio,
                        y: rect.y,
                        width: rect.width * (1.0 - ratio),
                        height: rect.height,
                    },
                ),
                Direction::Vertical => (
                    Rect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height * ratio,
                    },
                    Rect {
                        x: rect.x,
                        y: rect.y + rect.height * ratio,
                        width: rect.width,
                        height: rect.height * (1.0 - ratio),
                    },
                ),
            };
            collect_rects(first, first_rect, out);
            collect_rects(second, second_rect, out);
        }
    }
}

fn split_node(node: &mut Node, target: PaneId, direction: Direction, new_id: PaneId) -> bool {
    match node {
        Node::Pane(id) if *id == target => {
            let old_pane = Node::Pane(*id);
            let new_pane = Node::Pane(new_id);
            *node = Node::Split {
                direction,
                ratio: 0.5,
                first: Box::new(old_pane),
                second: Box::new(new_pane),
            };
            true
        }
        Node::Pane(_) => false,
        Node::Split { first, second, .. } => {
            split_node(first, target, direction, new_id)
                || split_node(second, target, direction, new_id)
        }
    }
}

fn close_node(node: &mut Node, target: PaneId) -> Option<PaneId> {
    match node {
        Node::Pane(id) if *id == target => None,
        Node::Pane(_) => None,
        Node::Split { first, second, .. } => {
            // Check if target is a direct child
            if let Node::Pane(id) = first.as_ref() {
                if *id == target {
                    let sibling = std::mem::replace(second.as_mut(), Node::Pane(PaneId(0)));
                    *node = sibling;
                    return find_first_pane(node);
                }
            }
            if let Node::Pane(id) = second.as_ref() {
                if *id == target {
                    let sibling = std::mem::replace(first.as_mut(), Node::Pane(PaneId(0)));
                    *node = sibling;
                    return find_first_pane(node);
                }
            }
            // Recurse
            close_node(first, target).or_else(|| close_node(second, target))
        }
    }
}

fn find_first_pane(node: &Node) -> Option<PaneId> {
    match node {
        Node::Pane(id) => Some(*id),
        Node::Split { first, .. } => find_first_pane(first),
    }
}

fn collect_panes(node: &Node, out: &mut Vec<PaneId>) {
    match node {
        Node::Pane(id) => out.push(*id),
        Node::Split { first, second, .. } => {
            collect_panes(first, out);
            collect_panes(second, out);
        }
    }
}

impl Default for LayoutTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tree_has_one_pane() {
        let tree = LayoutTree::new();
        assert_eq!(tree.panes().len(), 1);
    }

    #[test]
    fn split_creates_two_panes() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.split(first, Direction::Horizontal).unwrap();

        let panes = tree.panes();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&first));
        assert!(panes.contains(&second));
    }

    #[test]
    fn split_gives_equal_space() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.split(first, Direction::Horizontal).unwrap();

        let rects = tree.pane_rects();
        let first_rect = rects.get(&first).unwrap();
        let second_rect = rects.get(&second).unwrap();

        assert!((first_rect.width - 0.5).abs() < 0.001);
        assert!((second_rect.width - 0.5).abs() < 0.001);
    }

    #[test]
    fn close_removes_pane() {
        let mut tree = LayoutTree::new();
        let first = tree.focused_pane();
        let second = tree.split(first, Direction::Horizontal).unwrap();

        tree.close(second);

        let panes = tree.panes();
        assert_eq!(panes.len(), 1);
        assert!(panes.contains(&first));
    }
}
