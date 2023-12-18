use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

use num_bigint::BigUint;
use num_traits::Zero;

use super::Tree;

pub struct SuperficialTree {
    pub leaf_nodes: Vec<BigUint>,
    pub depth: u32,
    pub count: u64,
    pub zero_idxs: Vec<u64>,
}

impl SuperficialTree {
    pub fn new(depth: u32) -> Self {
        Self {
            leaf_nodes: vec![],
            depth,
            count: 0,
            zero_idxs: vec![],
        }
    }

    pub fn update_leaf_node(&mut self, leaf_hash: &BigUint, idx: u64) {
        assert!(idx < 2_u64.pow(self.depth), "idx is greater than tree size");

        if leaf_hash.ne(&BigUint::zero()) {
            if idx > self.count {
                for i in self.count..idx {
                    self.zero_idxs.push(i);
                }
                self.count = idx;
            } else if idx == self.count {
                self.count += 1;
            } else {
                self.zero_idxs = self
                    .zero_idxs
                    .iter()
                    .filter(|&x| *x != idx)
                    .map(|&x| x)
                    .collect::<Vec<u64>>();
            }
        } else {
            self.zero_idxs.push(idx);
        }

        if self.leaf_nodes.len() > idx as usize {
            self.leaf_nodes[idx as usize] = leaf_hash.clone();
        } else {
            let len_diff = idx as usize - self.leaf_nodes.len();

            for _ in 0..len_diff {
                self.leaf_nodes.push(BigUint::zero());
            }

            self.leaf_nodes.push(leaf_hash.clone())
        }
    }

    pub fn update_zero_idxs(&mut self) {
        self.zero_idxs = vec![];

        for (idx, leaf) in self.leaf_nodes.iter().enumerate() {
            if leaf.eq(&BigUint::zero()) {
                self.zero_idxs.push(idx as u64);
            }
        }
    }

    // -----------------------------------------------------------------

    pub fn from_disk() -> Result<SuperficialTree, Box<dyn std::error::Error>> {
        let str = "./storage/merkle_trees/state_tree".to_string();
        let dir = fs::read_dir(&str);

        let partitions = match dir {
            Ok(dir) => dir.count() - 1,
            // .filter(|entry| entry.as_ref().map(|e| e.path().is_dir()).unwrap_or(false))
            // .count(),
            Err(_) => 0,
        };

        if partitions == 0 {
            return Ok(SuperficialTree::new(32));
        }

        let mut leaf_nodes: Vec<BigUint> = Vec::new();
        let mut zero_idxs: Vec<u64> = Vec::new();
        for i in 0..partitions {
            let s = str.clone() + "/" + &i.to_string();

            let path = Path::new(&s);
            let mut file = File::open(path)?;
            let mut buf: Vec<u8> = Vec::new();

            file.read_to_end(&mut buf)?;

            let decoded: (Vec<Vec<u8>>, Vec<Vec<String>>, String, u32) =
                bincode::deserialize(&buf[..])?;

            decoded.0.iter().for_each(|x| {
                let x = BigUint::from_bytes_le(x);
                if x == BigUint::zero() {
                    zero_idxs.push(leaf_nodes.len() as u64);
                }

                leaf_nodes.push(x);
            });
        }

        let count = leaf_nodes.len() as u64;
        Ok(SuperficialTree {
            leaf_nodes,
            depth: 32,
            count,
            zero_idxs,
        })
    }

    // -----------------------------------------------------------------
    // * GETTERS * //
    pub fn first_zero_idx(&mut self) -> u64 {
        let idx;
        if self.zero_idxs.len() == 0 {
            idx = self.count;
            self.count += 1;
        } else {
            idx = self.zero_idxs.pop().unwrap_or_default();
        }

        if let Some(leaf) = self.leaf_nodes.get(idx as usize) {
            if *leaf != BigUint::zero() {
                return self.first_zero_idx();
            }
        }

        return idx;
    }

    pub fn get_leaf_by_index(&self, index: u64) -> BigUint {
        return self.nth_leaf_node(index);
    }

    // -----------------------------------------------------------------
    // Helpers

    fn nth_leaf_node(&self, n: u64) -> BigUint {
        assert!(n < 2_u64.pow(self.depth), "n is bigger than tree size");

        if self.leaf_nodes.get(n as usize).is_some() {
            return self.leaf_nodes[n as usize].clone();
        } else {
            return BigUint::zero();
        }
    }

    // -----------------------------------------------------------------

    pub fn from_tree(tree: Tree) -> Self {
        let count = tree.leaf_nodes.len() as u64;

        let mut zero_idxs = vec![];
        for (idx, leaf) in tree.leaf_nodes.iter().enumerate() {
            if leaf.eq(&BigUint::zero()) {
                zero_idxs.push(idx as u64);
            }
        }

        let superficial_tree = SuperficialTree {
            leaf_nodes: tree.leaf_nodes,
            depth: tree.depth,
            count,
            zero_idxs,
        };

        return superficial_tree;
    }
}
