//! A split tree of stable session identities, separate from panel docking.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::docking::{Edge, SplitAxis};

pub const MAX_PANES: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneMode {
    Chat,
    Terminal,
    #[default]
    Both,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node<K> {
    Session {
        session: K,
    },
    Split {
        axis: SplitAxis,
        sizes: [Option<f32>; 2],
        first: Box<Node<K>>,
        second: Box<Node<K>>,
    },
}

impl<K: Clone + PartialEq> Node<K> {
    pub fn sessions(&self) -> Vec<K> {
        match self {
            Self::Session { session } => vec![session.clone()],
            Self::Split { first, second, .. } => {
                let mut sessions = first.sessions();
                sessions.extend(second.sessions());
                sessions
            }
        }
    }

    pub fn contains(&self, id: &K) -> bool {
        match self {
            Self::Session { session } => session == id,
            Self::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    pub fn without(self, id: &K) -> Option<Self> {
        match self {
            Self::Session { ref session } => (session != id).then_some(self),
            Self::Split {
                axis,
                sizes,
                first,
                second,
            } => match (first.without(id), second.without(id)) {
                (Some(first), Some(second)) => Some(Self::split(axis, first, second, sizes)),
                (first, second) => first.or(second),
            },
        }
    }

    fn split(axis: SplitAxis, first: Self, second: Self, sizes: [Option<f32>; 2]) -> Self {
        Self::Split {
            axis,
            sizes,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn insert(&mut self, id: K, target: &K, edge: Edge) -> bool {
        match self {
            Self::Session { session } if session == target => {
                let old = Self::Session {
                    session: session.clone(),
                };
                let new = Self::Session { session: id };
                let axis = match edge {
                    Edge::Left | Edge::Right => SplitAxis::Horizontal,
                    Edge::Top | Edge::Bottom => SplitAxis::Vertical,
                };
                let (first, second) = match edge {
                    Edge::Left | Edge::Top => (new, old),
                    Edge::Right | Edge::Bottom => (old, new),
                };
                *self = Self::split(axis, first, second, [None, None]);
                true
            }
            Self::Split { first, second, .. } => {
                first.insert(id.clone(), target, edge) || second.insert(id, target, edge)
            }
            _ => false,
        }
    }

    pub fn map<L: Clone + PartialEq>(&self, f: &impl Fn(&K) -> Option<L>) -> Option<Node<L>> {
        match self {
            Self::Session { session } => f(session).map(|session| Node::Session { session }),
            Self::Split {
                axis,
                sizes,
                first,
                second,
            } => match (first.map(f), second.map(f)) {
                (Some(first), Some(second)) => Some(Node::split(*axis, first, second, *sizes)),
                (first, second) => first.or(second),
            },
        }
    }

    pub fn valid_sizes(&self) -> bool {
        match self {
            Self::Session { .. } => true,
            Self::Split {
                sizes,
                first,
                second,
                ..
            } => {
                sizes
                    .iter()
                    .flatten()
                    .all(|size| size.is_finite() && (48. ..=32_000.).contains(size))
                    && first.valid_sizes()
                    && second.valid_sizes()
            }
        }
    }

    /// Balanced rows, with the last row using all available width.
    fn grid(ids: &[K]) -> Self {
        let columns = (ids.len() as f32).sqrt().ceil() as usize;
        let rows: Vec<_> = ids
            .chunks(columns)
            .map(|row| Self::balanced(row, SplitAxis::Horizontal))
            .collect();
        Self::balanced_nodes(&rows, SplitAxis::Vertical)
    }

    fn balanced(ids: &[K], axis: SplitAxis) -> Self {
        let nodes: Vec<_> = ids
            .iter()
            .cloned()
            .map(|session| Self::Session { session })
            .collect();
        Self::balanced_nodes(&nodes, axis)
    }

    fn balanced_nodes(nodes: &[Self], axis: SplitAxis) -> Self {
        if nodes.len() == 1 {
            return nodes[0].clone();
        }
        let mid = nodes.len().div_ceil(2);
        Self::split(
            axis,
            Self::balanced_nodes(&nodes[..mid], axis),
            Self::balanced_nodes(&nodes[mid..], axis),
            [
                Some(100. * mid as f32),
                Some(100. * (nodes.len() - mid) as f32),
            ],
        )
    }
}

impl Node<u64> {
    pub fn key(&self) -> String {
        match self {
            Self::Session { session } => session.to_string(),
            Self::Split {
                axis,
                first,
                second,
                ..
            } => format!("{axis:?}({},{})", first.key(), second.key()),
        }
    }

    pub fn set_sizes(&mut self, key: &str, measured: [f32; 2]) -> bool {
        if self.key() == key
            && let Self::Split { sizes, .. } = self
        {
            if measured.iter().all(|n| n.is_finite() && *n >= 48.) {
                *sizes = measured.map(Some);
                return true;
            }
            return false;
        }
        match self {
            Self::Split { first, second, .. } => {
                first.set_sizes(key, measured) || second.set_sizes(key, measured)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub root: Option<Node<u64>>,
}

impl Layout {
    pub fn sessions(&self) -> Vec<u64> {
        self.root.as_ref().map_or_else(Vec::new, Node::sessions)
    }

    pub fn contains(&self, uid: u64) -> bool {
        self.root.as_ref().is_some_and(|root| root.contains(&uid))
    }

    pub fn tiled(&self) -> bool {
        matches!(self.root, Some(Node::Split { .. }))
    }

    pub fn split(&mut self, uid: u64, target: u64, edge: Edge) -> bool {
        if uid == target || !self.contains(target) {
            return false;
        }
        if !self.contains(uid) && self.sessions().len() >= MAX_PANES {
            return false;
        }
        let Some(mut root) = self.root.clone().and_then(|root| root.without(&uid)) else {
            return false;
        };
        if !root.insert(uid, &target, edge) {
            return false;
        }
        self.root = Some(root);
        true
    }

    pub fn remove(&mut self, uid: u64) -> bool {
        if !self.contains(uid) {
            return false;
        }
        self.root = self.root.take().and_then(|root| root.without(&uid));
        true
    }

    pub fn replace(&mut self, old: u64, new: u64) {
        if self.contains(new) {
            return;
        }
        self.root = self
            .root
            .as_ref()
            .and_then(|root| root.map(&|uid| Some(if *uid == old { new } else { *uid })));
    }

    pub fn grid(&mut self) {
        let ids = self.sessions();
        if !ids.is_empty() {
            self.root = Some(Node::grid(&ids));
        }
    }

    pub fn retain(&mut self, keep: impl Fn(u64) -> bool) {
        self.root = self
            .root
            .as_ref()
            .and_then(|root| root.map(&|uid| keep(*uid).then_some(*uid)));
    }
}

pub fn valid_saved(root: &Node<String>) -> bool {
    let ids = root.sessions();
    !ids.is_empty()
        && ids.len() <= MAX_PANES
        && ids.iter().all(|id| !id.is_empty())
        && ids.iter().collect::<HashSet<_>>().len() == ids.len()
        && root.valid_sizes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout {
            root: Some(Node::Session { session: 1 }),
        }
    }

    #[test]
    fn splits_move_existing_sessions_without_duplicates_and_collapse_on_close() {
        let mut layout = layout();
        assert!(layout.split(2, 1, Edge::Right));
        assert!(layout.split(3, 2, Edge::Bottom));
        assert_eq!(layout.sessions(), vec![1, 2, 3]);
        assert!(layout.split(1, 3, Edge::Left));
        assert_eq!(layout.sessions(), vec![2, 1, 3]);
        assert!(!layout.split(1, 1, Edge::Top));
        assert!(!layout.split(4, 99, Edge::Right));
        assert!(layout.remove(2));
        assert!(layout.remove(3));
        assert_eq!(layout.root, Some(Node::Session { session: 1 }));
    }

    #[test]
    fn grid_preserves_order_and_uses_two_rows_for_four_sessions() {
        let mut layout = layout();
        for uid in 2..=4 {
            assert!(layout.split(uid, uid - 1, Edge::Right));
        }
        layout.grid();
        assert_eq!(layout.sessions(), vec![1, 2, 3, 4]);
        let Some(Node::Split {
            axis,
            first,
            second,
            ..
        }) = layout.root
        else {
            panic!("rows")
        };
        assert_eq!(axis, SplitAxis::Vertical);
        assert_eq!(first.sessions(), vec![1, 2]);
        assert_eq!(second.sessions(), vec![3, 4]);
    }

    #[test]
    fn pruning_and_replacement_preserve_remaining_sessions() {
        let mut layout = layout();
        layout.split(2, 1, Edge::Right);
        layout.split(3, 2, Edge::Bottom);
        layout.replace(2, 4);
        layout.retain(|uid| uid != 1);
        assert_eq!(layout.sessions(), vec![4, 3]);
        let mapped = layout
            .root
            .unwrap()
            .map(&|id| (*id != 4).then(|| id.to_string()))
            .unwrap();
        assert_eq!(
            mapped,
            Node::Session {
                session: "3".into()
            }
        );
    }

    #[test]
    fn pane_limit_does_not_prevent_rearranging_existing_panes() {
        let mut layout = layout();
        for uid in 2..=MAX_PANES as u64 {
            assert!(layout.split(uid, uid - 1, Edge::Right));
        }
        assert!(!layout.split(99, 1, Edge::Right));
        assert!(layout.split(1, 2, Edge::Bottom));
        assert_eq!(layout.sessions().len(), MAX_PANES);
    }

    #[test]
    fn saved_layout_rejects_duplicate_ids_and_invalid_sizes() {
        let leaf = |id: &str| Node::Session {
            session: id.to_string(),
        };
        assert!(valid_saved(&leaf("a")));
        assert!(!valid_saved(&leaf("")));
        let mut root = Node::split(SplitAxis::Horizontal, leaf("a"), leaf("a"), [None, None]);
        assert!(!valid_saved(&root));
        root = Node::split(
            SplitAxis::Horizontal,
            leaf("a"),
            leaf("b"),
            [Some(f32::NAN), None],
        );
        assert!(!valid_saved(&root));
        if let Node::Split { sizes, .. } = &mut root {
            *sizes = [Some(120.), Some(240.)];
        }
        assert!(valid_saved(&root));
        let encoded = serde_json::to_string(&root).unwrap();
        assert_eq!(
            serde_json::from_str::<Node<String>>(&encoded).unwrap(),
            root
        );
    }

    #[test]
    fn resize_targets_the_right_branch_and_rejects_nonfinite_sizes() {
        let mut layout = layout();
        layout.split(2, 1, Edge::Right);
        layout.split(3, 2, Edge::Bottom);
        let root = layout.root.as_mut().unwrap();
        let key = match root {
            Node::Split { second, .. } => second.key(),
            _ => unreachable!(),
        };
        assert!(!root.set_sizes(&key, [f32::INFINITY, 100.]));
        assert!(!root.set_sizes("missing", [100., 100.]));
        assert!(root.set_sizes(&key, [200., 300.]));
        let Node::Split { second, .. } = root else {
            unreachable!()
        };
        let Node::Split { sizes, .. } = &**second else {
            unreachable!()
        };
        assert_eq!(*sizes, [Some(200.), Some(300.)]);
    }
}
