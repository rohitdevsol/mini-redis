pub struct AVLNode {
    pub depth: u32,
    pub cnt: u32,
    pub val: u32, // storing val directly for now
    pub left: Option<Box<AVLNode>>,
    pub right: Option<Box<AVLNode>>,
}

impl AVLNode {
    pub fn new(val: u32) -> Box<Self> {
        Box::new(AVLNode {
            depth: 1,
            cnt: 1,
            val,
            left: None,
            right: None,
        })
    }
}

fn depth(node: &Option<Box<AVLNode>>) -> u32 {
    node.as_ref().map_or(0, |n| n.depth)
}

fn cnt(node: &Option<Box<AVLNode>>) -> u32 {
    node.as_ref().map_or(0, |n| n.cnt)
}

fn update(node: &mut AVLNode) {
    node.depth = 1 + depth(&node.left).max(depth(&node.right));
    node.cnt = 1 + cnt(&node.left) + cnt(&node.right);
}

// ── rotations ────────────────────────────────────────────────────────────────

//      b                d
//     / \              / \
//    a   d    →       b   e
//       / \          / \
//      c   e        a   c
fn rot_left(mut node: Box<AVLNode>) -> Box<AVLNode> {
    let mut new_root = node.right.take().unwrap();
    node.right = new_root.left.take(); // c moves from d's left to b's right
    update(&mut node);
    new_root.left = Some(node); // b becomes d's left child
    update(&mut new_root);
    new_root
}

//        d                b
//       / \              / \
//      b   e    →       a   d
//     / \                  / \
//    a   c                c   e
fn rot_right(mut node: Box<AVLNode>) -> Box<AVLNode> {
    let mut new_root = node.left.take().unwrap();
    node.left = new_root.right.take(); // c moves from b's right to d's left
    update(&mut node);
    new_root.right = Some(node); // d becomes b's right child
    update(&mut new_root);
    new_root
}

// ── fixing imbalance ──────────────────────────────────────────────────────────

// left subtree is too deep by 2
fn fix_left(mut node: Box<AVLNode>) -> Box<AVLNode> {
    // check if we need a double rotation (left-right case)
    if depth(&node.left.as_ref().unwrap().left) < depth(&node.left.as_ref().unwrap().right) {
        // left child is right-heavy → rotate it left first
        node.left = Some(rot_left(node.left.take().unwrap()));
    }
    // now rotate right
    rot_right(node)
}

// right subtree is too deep by 2
fn fix_right(mut node: Box<AVLNode>) -> Box<AVLNode> {
    // check if we need a double rotation (right-left case)
    if depth(&node.right.as_ref().unwrap().right) < depth(&node.right.as_ref().unwrap().left) {
        // right child is left-heavy → rotate it right first
        node.right = Some(rot_right(node.right.take().unwrap()));
    }
    // now rotate left
    rot_left(node)
}

// rebalance a node after update
// checks balance factor and applies the right fix
fn fix(mut node: Box<AVLNode>) -> Box<AVLNode> {
    update(&mut node);
    let l = depth(&node.left);
    let r = depth(&node.right);

    if l == r + 2 {
        fix_left(node) // left too heavy
    } else if l + 2 == r {
        fix_right(node) // right too heavy
    } else {
        node // balanced, nothing to do
    }
}

// ── insert ────────────────────────────────────────────────────────────────────

pub fn insert(node: Option<Box<AVLNode>>, val: u32) -> Box<AVLNode> {
    match node {
        None => AVLNode::new(val), // empty spot found, place here
        Some(mut n) => {
            if val < n.val {
                n.left = Some(insert(n.left.take(), val));
            } else {
                n.right = Some(insert(n.right.take(), val));
            }
            fix(n) // rebalance on the way back up the recursion
        }
    }
}

// ── delete ────────────────────────────────────────────────────────────────────

// find and remove the minimum node in a subtree
// returns (new subtree root, the removed min node's value)
fn remove_min(mut node: Box<AVLNode>) -> (Option<Box<AVLNode>>, u32) {
    if node.left.is_none() {
        let val = node.val; // copy first (u32 is Copy)
        let right = node.right.take(); // then take right
        return (right, val);
    }
    let (new_left, min_val) = remove_min(node.left.take().unwrap());
    node.left = new_left;
    (Some(fix(node)), min_val)
}

pub fn delete(node: Option<Box<AVLNode>>, val: u32) -> (Option<Box<AVLNode>>, bool) {
    match node {
        None => (None, false), // val not found
        Some(mut n) => {
            if val < n.val {
                // go left
                let (new_left, deleted) = delete(n.left.take(), val);
                n.left = new_left;
                (Some(fix(n)), deleted)
            } else if val > n.val {
                // go right
                let (new_right, deleted) = delete(n.right.take(), val);
                n.right = new_right;
                (Some(fix(n)), deleted)
            } else {
                // found the node to delete
                match (n.left.take(), n.right.take()) {
                    (None, right) => (right, true), // no left child, replace with right
                    (left, None) => (left, true), // no right child, replace with left

                    (left, right) => {
                        // has both children
                        // find in-order successor (min of right subtree)
                        // replace current val with successor val
                        // delete successor from right subtree
                        let (new_right, successor_val) = remove_min(right.unwrap());
                        n.val = successor_val;
                        n.left = left;
                        n.right = new_right;
                        (Some(fix(n)), true)
                    }
                }
            }
        }
    }
}

// src/avl.rs continued

pub fn verify(node: &Option<Box<AVLNode>>) {
    let n = match node {
        None => {
            return;
        }
        Some(n) => n,
    };

    // recursively verify children first
    verify(&n.left);
    verify(&n.right);

    // depth must be correct
    let l = depth(&n.left);
    let r = depth(&n.right);
    assert_eq!(n.depth, 1 + l.max(r), "depth wrong at node {}", n.val);

    // cnt must be correct
    assert_eq!(n.cnt, 1 + cnt(&n.left) + cnt(&n.right), "cnt wrong at node {}", n.val);

    // AVL balance property — diff never more than 1
    let diff = ((l as i32) - (r as i32)).abs();
    assert!(diff <= 1, "balance violated at node {}: l={} r={}", n.val, l, r);

    // BST ordering property
    if let Some(left) = &n.left {
        assert!(left.val <= n.val, "BST violated: left {} > parent {}", left.val, n.val);
    }
    if let Some(right) = &n.right {
        assert!(right.val >= n.val, "BST violated: right {} < parent {}", right.val, n.val);
    }
}

// collect all values in sorted order (in-order traversal)
pub fn collect(node: &Option<Box<AVLNode>>, out: &mut Vec<u32>) {
    if let Some(n) = node {
        collect(&n.left, out);
        out.push(n.val);
        collect(&n.right, out);
    }
}
