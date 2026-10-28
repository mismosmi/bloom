use std::{collections::HashMap, mem};

use futures_util::task::Spawn;

use crate::{commit::Changes, render_context::RenderContext, Element, Key};

use super::{
    chain,
    tree::{element_key, TreeNode},
    visit::{remove, sibling_anchors, visit, visit_all},
    Pass, Visited, Work,
};

/// Children are matched by key if any of them has one and by position otherwise.
pub(super) async fn update_children<N, E, S>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    elements: Vec<Element<N, E>>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    if tree_nodes.iter().any(|node| node.key().is_some())
        || elements
            .iter()
            .any(|element| element_key(element).is_some())
    {
        update_keyed_children(tree_nodes, elements, ctx, pass).await
    } else {
        update_children_by_position(tree_nodes, elements, ctx, pass).await
    }
}

pub(super) async fn update_children_by_position<N, E, S>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    mut elements: Vec<Element<N, E>>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let kept = elements.len().min(tree_nodes.len());
    let removed: Vec<TreeNode<N, E>> = tree_nodes.drain(kept..).collect();
    tree_nodes.shrink_to_fit();
    tree_nodes.extend(elements.split_off(kept).into_iter().map(TreeNode::from));

    // New children are appended before the sibling of the whole list
    let anchors = sibling_anchors(&tree_nodes[..kept], ctx.sibling.clone());
    let mut updates = elements.into_iter().zip(anchors);
    let visited = visit_all(
        tree_nodes
            .iter_mut()
            .map(|tree_node| match updates.next() {
                Some((element, sibling)) => visit(
                    tree_node,
                    Work::Update(element),
                    ctx.with_sibling(sibling),
                    pass,
                ),
                None => visit(tree_node, Work::Create, ctx.clone(), pass),
            })
            .collect(),
    )
    .await;

    // Removed children are not in the tree anymore, so they are removed even if a child failed
    let mut changes = Changes::default();
    for tree_node in removed {
        remove(tree_node, ctx.attached_parent(), &mut changes);
    }
    chain(visited, changes)
}

/// Identifies a child in a keyed list.
/// Children without a key are identified by their position among the other unkeyed children.
#[derive(PartialEq, Eq, Hash)]
pub(super) enum Slot {
    Key(Key),
    Index(usize),
}

pub(super) fn slots(keys: impl Iterator<Item = Option<Key>>) -> impl Iterator<Item = Slot> {
    let mut index = 0;
    keys.map(move |key| match key {
        Some(key) => Slot::Key(key),
        None => {
            index += 1;
            Slot::Index(index - 1)
        }
    })
}

pub(super) async fn update_keyed_children<N, E, S>(
    tree_nodes: &mut Vec<TreeNode<N, E>>,
    elements: Vec<Element<N, E>>,
    ctx: RenderContext<N>,
    pass: &Pass<S>,
) -> Visited<N, E>
where
    N: From<String> + Send + Sync + 'static,
    E: Send + 'static,
    S: Spawn + Clone + Send + Sync + 'static,
{
    let old_nodes = mem::take(tree_nodes);
    let mut old_indices = HashMap::with_capacity(old_nodes.len());
    for (index, slot) in slots(old_nodes.iter().map(TreeNode::key)).enumerate() {
        // Only the first of several children with the same key is matched
        old_indices.entry(slot).or_insert(index);
    }
    let mut old_nodes: Vec<Option<TreeNode<N, E>>> = old_nodes.into_iter().map(Some).collect();

    // For each child in the new order, the old index and next element of a matched child
    // or None for a new child
    let new_slots: Vec<Slot> = slots(elements.iter().map(element_key)).collect();
    let mut matches = Vec::with_capacity(elements.len());
    for (element, slot) in elements.into_iter().zip(new_slots) {
        match old_indices.remove(&slot) {
            Some(old_index) => {
                tree_nodes.push(old_nodes[old_index].take().expect("Child matched twice"));
                matches.push(Some((old_index, element)));
            }
            None => {
                tree_nodes.push(TreeNode::from(element));
                matches.push(None);
            }
        }
    }

    // Matched children are moved first so the new children and replaced nodes can be
    // inserted relative to them later.
    // Going from the end, every child that is part of the longest increasing subsequence
    // of old indices is already in place, every other one is moved before the next matched child.
    let mut changes = Changes::default();
    let old_order: Vec<usize> = matches
        .iter()
        .flatten()
        .map(|(old_index, _)| *old_index)
        .collect();
    let mut in_place = longest_increasing_subsequence(&old_order).into_iter().rev();
    let mut anchor = ctx.sibling.clone();
    let mut nodes = Vec::new();
    for (tree_node, _) in tree_nodes
        .iter()
        .zip(&matches)
        .rev()
        .filter(|(_, matched)| matched.is_some())
    {
        // Detached nodes are not in the object model yet, they are mounted in order later
        if !in_place.next().expect("Missing matched child") && !ctx.detached {
            nodes.clear();
            tree_node.collect_nodes(&mut nodes);
            for node in &nodes {
                changes.move_node(node, &ctx.parent, &anchor);
            }
        }
        if let Some(first_node) = tree_node.get_first_node() {
            anchor = Some(first_node);
        }
    }

    // New children are not created yet, so only matched children can be siblings.
    // Children are visited front to back, so new children that share a sibling
    // are still inserted in order.
    let mut anchors = Vec::with_capacity(tree_nodes.len());
    let mut sibling = ctx.sibling.clone();
    for (tree_node, matched) in tree_nodes.iter().zip(&matches).rev() {
        anchors.push(sibling.clone());
        if matched.is_some() {
            if let Some(first_node) = tree_node.get_first_node() {
                sibling = Some(first_node);
            }
        }
    }
    anchors.reverse();

    let visited = visit_all(
        tree_nodes
            .iter_mut()
            .zip(matches)
            .zip(anchors)
            .map(|((tree_node, matched), sibling)| {
                let work = match matched {
                    Some((_, element)) => Work::Update(element),
                    None => Work::Create,
                };
                visit(tree_node, work, ctx.with_sibling(sibling), pass)
            })
            .collect(),
    )
    .await;

    // Removed children are not in the tree anymore, so they are removed even if a child failed
    let mut removed = Changes::default();
    for old_node in old_nodes.into_iter().flatten() {
        remove(old_node, ctx.attached_parent(), &mut removed);
    }
    changes.merge(chain(visited, removed))?;
    Ok(changes)
}

/// Marks the values that are part of a longest strictly increasing subsequence.
pub(super) fn longest_increasing_subsequence(values: &[usize]) -> Vec<bool> {
    // tails[k] is the index of the smallest last value of an increasing subsequence of length k + 1
    let mut tails: Vec<usize> = Vec::new();
    let mut predecessors = vec![None; values.len()];
    for (index, value) in values.iter().enumerate() {
        let length = tails.partition_point(|&tail| values[tail] < *value);
        if length > 0 {
            predecessors[index] = Some(tails[length - 1]);
        }
        if length == tails.len() {
            tails.push(index);
        } else {
            tails[length] = index;
        }
    }

    let mut in_sequence = vec![false; values.len()];
    let mut current = tails.last().copied();
    while let Some(index) = current {
        in_sequence[index] = true;
        current = predecessors[index];
    }
    in_sequence
}

#[cfg(test)]
mod tests {
    #[test]
    fn longest_increasing_subsequence() {
        assert_eq!(
            super::longest_increasing_subsequence(&[]),
            Vec::<bool>::new()
        );
        assert_eq!(
            super::longest_increasing_subsequence(&[4, 0, 1, 2, 3]),
            vec![false, true, true, true, true]
        );
        assert_eq!(
            super::longest_increasing_subsequence(&[0, 4, 3, 2, 1])
                .into_iter()
                .filter(|in_sequence| *in_sequence)
                .count(),
            2
        );
        assert_eq!(
            super::longest_increasing_subsequence(&[2, 0, 3, 1, 4]),
            vec![false, true, false, true, true]
        );
    }
}
