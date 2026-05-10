use mini_redis::avl::{ insert, delete, verify, collect };

use std::collections::BTreeSet;

fn main() {
    println!("running AVL tree tests...");

    // ── test 1: basic insert and delete
    let mut root = None;
    verify(&root);

    root = Some(insert(root, 123));
    verify(&root);
    let mut vals = vec![];
    collect(&root, &mut vals);
    assert_eq!(vals, vec![123]);

    let (new_root, deleted) = delete(root, 999);
    root = new_root;
    assert!(!deleted); // 999 was never inserted

    let (new_root, deleted) = delete(root, 123);
    root = new_root;
    assert!(deleted);
    verify(&root);
    assert!(root.is_none());

    println!("  basic tests passed");

    // ── test 2: sequential insertion
    // this is the case that breaks a plain BST but AVL handles fine
    let mut root = None;
    let mut reference = BTreeSet::new(); // Rust's sorted set for comparison

    for i in (0..1000u32).step_by(3) {
        root = Some(insert(root, i));
        reference.insert(i);
        verify(&root);

        let mut vals = vec![];
        collect(&root, &mut vals);
        let ref_vals: Vec<u32> = reference.iter().cloned().collect();
        assert_eq!(vals, ref_vals, "mismatch after inserting {}", i);
    }

    println!("  sequential insertion passed ({} nodes)", reference.len());

    // ── test 3: random insertion and deletion
    let mut root = None;
    let mut reference = BTreeSet::new();

    // random inserts
    for i in 0..100u32 {
        let val = (i * 17 + 3) % 1000; // pseudo-random
        root = Some(insert(root, val));
        reference.insert(val);
        verify(&root);
    }

    // random deletes
    for i in 0..200u32 {
        let val = (i * 13 + 7) % 1000;
        let (new_root, deleted) = delete(root, val);
        root = new_root;
        let was_there = reference.remove(&val);
        assert_eq!(deleted, was_there, "delete mismatch for val {}", val);
        if root.is_some() {
            verify(&root);
        }
    }

    println!("  random insert/delete passed");

    // ── test 4: insert at every position
    for sz in 0..50u32 {
        for val in 0..sz {
            let mut root = None;
            for i in 0..sz {
                if i != val {
                    root = Some(insert(root, i));
                }
            }
            verify(&root);
            root = Some(insert(root, val));
            verify(&root);

            let mut got = vec![];
            collect(&root, &mut got);
            let expected: Vec<u32> = (0..sz).collect();
            assert_eq!(got, expected);
        }
    }

    println!("  insert-at-every-position passed");

    // ── test 5: delete at every position
    for sz in 1..50u32 {
        for val in 0..sz {
            let mut root = None;
            for i in 0..sz {
                root = Some(insert(root, i));
            }
            let (new_root, deleted) = delete(root, val);
            root = new_root;
            assert!(deleted);
            verify(&root);

            let mut got = vec![];
            collect(&root, &mut got);
            let expected: Vec<u32> = (0..sz).filter(|&i| i != val).collect();
            assert_eq!(got, expected);
        }
    }

    println!("  delete-at-every-position passed");
    println!("all tests passed ✓");
}
