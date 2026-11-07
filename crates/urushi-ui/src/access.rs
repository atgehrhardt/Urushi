//! Accessibility: turns a frame's [`AccessNode`]s into an AccessKit tree update, so
//! screen readers can see and press the UI.

use accesskit::{
    Action, Node as AkNode, NodeId, Rect as AkRect, Role as AkRole, TreeId, TreeInfo, TreeUpdate,
};

use crate::layout::AccessNode;
use crate::node::{Key, Role};

/// Id of the root window node.
pub const ROOT_ID: NodeId = NodeId(0);

fn role(role: Role) -> AkRole {
    match role {
        Role::None => AkRole::GenericContainer,
        Role::Button => AkRole::Button,
        Role::Label => AkRole::Label,
        Role::TextInput => AkRole::TextInput,
        Role::Item => AkRole::ListItem,
        Role::Dialog => AkRole::Dialog,
        Role::Terminal => AkRole::Terminal,
        Role::Switch => AkRole::Switch,
    }
}

/// A full tree update: a window root whose children are `nodes`. `scale` converts
/// logical pixels to the physical pixels AccessKit expects.
pub fn tree_update(
    window_title: &str,
    nodes: &[AccessNode],
    focus: Option<Key>,
    scale: f64,
) -> TreeUpdate {
    let mut root = AkNode::new(AkRole::Window);
    root.set_label(window_title);
    let mut out = Vec::with_capacity(nodes.len() + 1);
    let mut children = Vec::with_capacity(nodes.len());
    for n in nodes {
        // Keep 0 for the root.
        let id = NodeId(n.id.max(1));
        let mut node = AkNode::new(role(n.role));
        node.set_label(n.label.clone());
        node.set_bounds(AkRect {
            x0: f64::from(n.rect.x) * scale,
            y0: f64::from(n.rect.y) * scale,
            x1: f64::from(n.rect.right()) * scale,
            y1: f64::from(n.rect.bottom()) * scale,
        });
        if n.clickable {
            node.add_action(Action::Click);
        }
        node.add_action(Action::Focus);
        children.push(id);
        out.push((id, node));
    }
    root.set_children(children);
    out.insert(0, (ROOT_ID, root));
    let focused = focus
        .map(|k| NodeId(k.max(1)))
        .filter(|id| out.iter().any(|(n, _)| n == id))
        .unwrap_or(ROOT_ID);
    TreeUpdate {
        nodes: out,
        tree: Some(TreeInfo::new(ROOT_ID)),
        tree_id: TreeId::ROOT,
        focus: focused,
    }
}
