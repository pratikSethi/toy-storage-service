//! Where a chunk goes: rendezvous (highest random weight) hashing.
//!
//! Every placeable node gets a score for the chunk, `sha256(node_id ":" chunk_id)`, and the chunk goes to the
//! highest scores. Adding or removing a node only moves the chunks whose top score was on that node, and
//! nothing needs to be stored: the same node list gives the same answer.

use sha2::{Digest, Sha256};

fn score(node: &str, chunk_id: &str) -> u64 {
    let mut h = Sha256::new();
    h.update(node.as_bytes());
    h.update(b":");
    h.update(chunk_id.as_bytes());
    u64::from_be_bytes(h.finalize()[..8].try_into().unwrap())
}

/// The `r` best nodes for `chunk_id`, best first (fewer if there are fewer nodes). Callers pass only the
/// placeable nodes, so an unhealthy node is skipped by leaving it out. R=1 uses `[0]`; replication will use more.
pub fn rendezvous(chunk_id: &str, nodes: &[String], r: usize) -> Vec<String> {
    let mut scored: Vec<_> = nodes.iter().map(|n| (score(n, chunk_id), n)).collect();
    // Highest score first; the node id breaks the (practically impossible) tie so the order is total.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.into_iter().take(r).map(|(_, n)| n.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("node-{i}")).collect()
    }

    fn ids() -> impl Iterator<Item = String> {
        (0..4000u64).map(|i| format!("{:032x}", u128::from(i) * 0x9e37_79b9_7f4a_7c15))
    }

    #[test]
    fn spreads_chunks_roughly_evenly_over_four_nodes() {
        let ns = nodes(4);
        let mut count = std::collections::HashMap::new();
        for id in ids() {
            *count.entry(rendezvous(&id, &ns, 1).remove(0)).or_insert(0) += 1;
        }
        assert_eq!(count.len(), 4);
        // 4000 chunks, 1000 expected per node; allow +-20%.
        for (n, c) in count {
            assert!((800..=1200).contains(&c), "{n} got {c}");
        }
    }

    #[test]
    fn removing_a_node_only_moves_the_chunks_it_held() {
        let all = nodes(4);
        let without: Vec<_> = all.iter().filter(|n| *n != "node-3").cloned().collect();
        for id in ids() {
            let before = rendezvous(&id, &all, 1).remove(0);
            let after = rendezvous(&id, &without, 1).remove(0);
            if before != "node-3" {
                assert_eq!(
                    before, after,
                    "chunk {id} moved though node-3 did not hold it"
                );
            } else {
                assert_ne!(after, "node-3");
            }
        }
    }

    #[test]
    fn an_unhealthy_node_left_out_of_the_list_gets_nothing() {
        let healthy: Vec<_> = nodes(4).into_iter().filter(|n| n != "node-2").collect();
        assert!(ids().all(|id| rendezvous(&id, &healthy, 3).iter().all(|n| n != "node-2")));
    }

    #[test]
    fn returns_an_ordered_list_whose_first_entry_is_the_r1_answer() {
        let ns = nodes(4);
        for id in ids().take(200) {
            let three = rendezvous(&id, &ns, 3);
            assert_eq!(three.len(), 3);
            assert_eq!(three[0], rendezvous(&id, &ns, 1)[0]);
            assert_eq!(rendezvous(&id, &ns, 9).len(), 4);
        }
    }
}
