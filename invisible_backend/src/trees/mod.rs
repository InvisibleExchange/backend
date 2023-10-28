use std::{
    collections::HashMap,
    error::Error,
    fmt::Debug,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    str::FromStr,
    sync::Arc,
};

use num_bigint::BigUint;
use num_traits::Zero;

use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::trees::tree_utils::get_zero_hash;
use crate::utils::crypto_utils::pedersen;

pub mod superficial_tree;
mod tree_utils;

#[derive(Debug, Clone)]
pub struct Tree {
    pub leaf_nodes: Vec<BigUint>,
    pub inner_nodes: Vec<Vec<BigUint>>,
    pub depth: u32,
    pub root: BigUint,
    pub shift: u32, // in case of a root tree we can start at a different depth
}

impl Tree {
    pub fn new(depth: u32, shift: u32) -> Tree {
        let leaf_nodes: Vec<BigUint> = Vec::new();
        let mut inner_nodes: Vec<Vec<BigUint>> = Vec::new();
        let root = get_zero_hash(depth, shift);

        for _ in 0..depth {
            let empty_vec: Vec<BigUint> = Vec::new();
            inner_nodes.push(empty_vec);
        }

        return Tree {
            leaf_nodes,
            inner_nodes,
            depth,
            root,
            shift,
        };
    }

    // -----------------------------------------------------------------
    // Optimized parallel transition from one tx_batch to another
    // Updates the tree with a batch of updates and generates the preimage multi update proofs

    pub fn batch_transition_updates(
        &mut self,
        update_proofs: &HashMap<u64, BigUint>,
        preimage: &mut Map<String, Value>,
    ) {
        //

        if update_proofs.len() == 0 {
            return;
        }

        let tree_depth = self.depth;

        let tree_mutex = Arc::new(Mutex::new(self));
        let preimage_mutex = Arc::new(Mutex::new(preimage));

        let mut next_row = split_and_run_first_row(&tree_mutex, &preimage_mutex, update_proofs, 0);

        for i in 1..tree_depth as usize {
            next_row = split_and_run_next_row(&tree_mutex, &preimage_mutex, &next_row, i, 0);
        }

        let mut tree = tree_mutex.lock();
        tree.root = tree.inner_nodes[tree_depth as usize - 1][0].clone();
        drop(tree);
    }

    // -----------------------------------------------------------------
    // HELPERS

    fn update_leaf_node(&mut self, leaf_hash: &BigUint, idx: u64) {
        assert!(idx < 2_u64.pow(self.depth), "idx is greater than tree size");

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

    fn update_inner_node(&mut self, i: u32, j: u64, value: BigUint) {
        assert!(i <= self.depth, "i is greater than depth");
        assert!(j < 2_u64.pow(self.depth - i), "j is greater than 2^i");

        if self.inner_nodes.get(i as usize - 1).unwrap().len() > j as usize {
            self.inner_nodes[i as usize - 1][j as usize] = value;
        } else {
            let len_diff = j as usize - self.inner_nodes[i as usize - 1].len();

            for _ in 0..len_diff {
                self.inner_nodes[i as usize - 1].push(get_zero_hash(i, self.shift));
            }

            self.inner_nodes[i as usize - 1].push(value);
        }
    }

    fn nth_leaf_node(&self, n: u64) -> BigUint {
        assert!(n < 2_u64.pow(self.depth), "n is bigger than tree size");

        if self.leaf_nodes.get(n as usize).is_some() {
            return self.leaf_nodes[n as usize].clone();
        } else {
            return get_zero_hash(0, self.shift);
        }
    }

    fn ith_inner_node(&self, i: u32, j: u64) -> BigUint {
        // ? Checks if the inner note at that spot exists, else it returns the zero hash

        assert!(i <= self.depth, "i is greater than depth");
        assert!(j < 2_u64.pow(self.depth - i), "j is greater than 2^i");

        if self.inner_nodes.get(i as usize - 1).is_some()
            && self.inner_nodes[i as usize - 1].get(j as usize).is_some()
        {
            let res = self.inner_nodes[i as usize - 1][j as usize].clone();
            return res;
        } else {
            let zero_hash = get_zero_hash(i, self.shift);
            return zero_hash;
        }
    }

    // I/O Operations --------------------------------------------------

    pub fn store_to_disk(&self, tree_index: u32, is_backup: bool) -> Result<(), Box<dyn Error>> {
        let str: String;
        if is_backup {
            str = "./storage/merkle_trees/state_tree_backup/".to_string() + &tree_index.to_string();
        } else {
            str = "./storage/merkle_trees/state_tree/".to_string() + &tree_index.to_string();
        }

        let path = Path::new(&str);
        if is_backup {
            if !Path::new("./storage/merkle_trees/state_tree_backup/").exists() {
                fs::create_dir("./storage/merkle_trees/state_tree_backup/")?;
            }
        } else {
            if !Path::new("./storage/merkle_trees/state_tree/").exists() {
                fs::create_dir("./storage/merkle_trees/state_tree/")?;
            }
        }

        let mut file: File = File::create(path)?;

        let leaves = &self
            .leaf_nodes
            .iter()
            .map(|x| x.to_bytes_le())
            .collect::<Vec<Vec<u8>>>();

        let inner_nodes = self
            .inner_nodes
            .iter()
            .map(|x| x.iter().map(|y| y.to_string()).collect::<Vec<String>>())
            .collect::<Vec<Vec<String>>>();

        let encoded: Vec<u8> =
            bincode::serialize(&(leaves, inner_nodes, self.root.to_string(), self.depth)).unwrap();

        file.write_all(&encoded[..])?;

        Ok(())
    }

    pub fn from_disk(tree_index: u32, depth: u32, shift: u32) -> Result<Tree, Box<dyn Error>> {
        let str = "./storage/merkle_trees/state_tree/";
        let path_str = str.to_string() + &tree_index.to_string();
        let path = Path::new(&path_str);

        let open_res = File::open(path).ok();
        if open_res.is_none() {
            if Path::new(&str).exists() {
                File::create(path)?;
                return Ok(Tree::new(depth, shift));
            } else {
                fs::create_dir(&str)?;
                File::create(path)?;
                return Ok(Tree::new(depth, shift));
            }
        };

        let mut file: File = open_res.unwrap();
        let mut buf: Vec<u8> = Vec::new();

        file.read_to_end(&mut buf)?;

        let decoded: (Vec<Vec<u8>>, Vec<Vec<String>>, String, u32) =
            bincode::deserialize(&buf[..])?;

        let leaves = decoded
            .0
            .iter()
            .map(|x| BigUint::from_bytes_le(x))
            .collect();
        let inner_nodes = decoded
            .1
            .iter()
            .map(|x| {
                x.iter()
                    .map(|y| BigUint::from_str(y.as_str()).unwrap())
                    .collect::<Vec<BigUint>>()
            })
            .collect::<Vec<Vec<BigUint>>>();

        Ok(Tree {
            leaf_nodes: leaves,
            inner_nodes,
            root: BigUint::from_str(&decoded.2.as_str()).unwrap(),
            depth: decoded.3,
            shift,
        })
    }

    // -----------------------------------------------------------------

    pub fn get_proof(&self, leaf_idx: u64) -> (Vec<BigUint>, Vec<i8>) {
        let proof_binary_pos = tree_utils::idx_to_binary_pos(leaf_idx, self.depth as usize);

        let proof_pos = tree_utils::proof_pos(leaf_idx, self.depth as usize);

        let mut proof: Vec<BigUint> = Vec::new();
        proof.push(self.nth_leaf_node(proof_pos[0]));

        for i in 1..self.depth {
            let proof_val = self.ith_inner_node(i, proof_pos[i as usize] as u64);

            proof.push(proof_val);
        }

        return (proof, proof_binary_pos);
    }

    // -----------------------------------------------------------------
    // ! For Testing
    pub fn verify_root(&self) -> bool {
        let leaf_nodes = pad_leaf_nodes(&self.leaf_nodes, self.depth as usize, BigUint::zero());

        let inner_nodes: Vec<Vec<BigUint>> =
            inner_from_leaf_nodes(self.depth as usize, &leaf_nodes);
        let root = inner_nodes[0][0].clone();

        return self.root == root;
    }
}

fn inner_from_leaf_nodes(depth: usize, leaf_nodes: &Vec<BigUint>) -> Vec<Vec<BigUint>> {
    let mut tree: Vec<Vec<BigUint>> = Vec::new();

    // if leaf_nodes.len() % 2 == 1 {
    //     leaf_nodes.push(BigUint::from_i8(0).unwrap());
    // }

    let mut hashes: Vec<BigUint> = tree_utils::pairwise_hash(&leaf_nodes);

    tree.push(hashes.clone());

    for _ in 0..depth - 1 {
        hashes = tree_utils::pairwise_hash(&hashes);
        tree.push(hashes.clone());
    }

    tree.reverse();
    return tree;
}

fn pad_leaf_nodes(arr: &Vec<BigUint>, depth: usize, pad_value: BigUint) -> Vec<BigUint> {
    let total_len = 2_usize.pow(depth as u32);
    let mut new_arr: Vec<BigUint> = arr.clone();
    for _ in 0..total_len - arr.len() {
        new_arr.push(pad_value.clone());
    }

    return new_arr;
}

// * =================================================================================================================
// * HELPER FUNCTION FOR PARALLEL UPDATES

const STRIDE: usize = 250; // Must be even

fn split_and_run_first_row(
    tree_mutex: &Arc<Mutex<&mut Tree>>,
    preimage_mutex: &Arc<Mutex<&mut Map<String, Value>>>,
    update_proofs: &HashMap<u64, BigUint>,
    n: usize,
) -> HashMap<u64, BigUint> {
    let next_row_proofs: HashMap<u64, BigUint> = HashMap::new();
    let next_row_proofs_mutex = Arc::new(Mutex::new(next_row_proofs));

    split_and_run_first_row_inner(
        tree_mutex,
        preimage_mutex,
        update_proofs,
        &next_row_proofs_mutex,
        n,
    );

    let res = next_row_proofs_mutex.lock();
    return res.to_owned();
}

fn split_and_run_first_row_inner(
    tree_mutex: &Arc<Mutex<&mut Tree>>,
    preimage_mutex: &Arc<Mutex<&mut Map<String, Value>>>,
    update_proofs: &HashMap<u64, BigUint>,
    next_row: &Arc<Mutex<HashMap<u64, BigUint>>>,
    n: usize,
) {
    // ? n counts how deep in the recursion loop we are
    // ? at each iteration we take four elements from the hashmap and update the tree

    let elems: Vec<(&u64, &BigUint)> = update_proofs.iter().skip(n * STRIDE).take(STRIDE).collect();

    // ? As long as there are elements in the map (elems.len() > 0) we keep splitting
    // ? Pass the rest forward recursively to run in parallel
    if elems.len() > 0 {
        rayon::join(
            || {
                let next_row_indexes =
                    build_first_row(tree_mutex, preimage_mutex, elems, update_proofs);
                let mut next_proofs = next_row.lock();
                for (i, prev_res) in next_row_indexes {
                    next_proofs.insert(i, prev_res);
                }
                drop(next_proofs);
            },
            || {
                split_and_run_first_row_inner(
                    tree_mutex,
                    preimage_mutex,
                    update_proofs,
                    next_row,
                    n + 1,
                )
            },
        );
    }
}

// ------------------------------

fn split_and_run_next_row(
    tree_mutex: &Arc<Mutex<&mut Tree>>,
    preimage_mutex: &Arc<Mutex<&mut Map<String, Value>>>,
    update_proofs: &HashMap<u64, BigUint>,
    row_depth: usize,
    n: usize,
) -> HashMap<u64, BigUint> {
    let next_row_proofs: HashMap<u64, BigUint> = HashMap::new();
    let next_row_proofs_mutex = Arc::new(Mutex::new(next_row_proofs));

    split_and_run_next_row_inner(
        tree_mutex,
        preimage_mutex,
        update_proofs,
        &next_row_proofs_mutex,
        row_depth,
        n,
    );

    let res = next_row_proofs_mutex.lock();
    return res.to_owned();
}

fn split_and_run_next_row_inner(
    tree_mutex: &Arc<Mutex<&mut Tree>>,
    preimage_mutex: &Arc<Mutex<&mut Map<String, Value>>>,
    update_proofs: &HashMap<u64, BigUint>,
    next_row: &Arc<Mutex<HashMap<u64, BigUint>>>,
    row_depth: usize,
    n: usize,
) {
    // ? n counts how deep in the recursion loop we are
    // ? at each iteration we take four elements from the hashmap and update the tree

    let elems: Vec<(&u64, &BigUint)> = update_proofs.iter().skip(n * STRIDE).take(STRIDE).collect();

    // ? As long as there are elements in the map (elems.len() > 0) we keep splitting
    // ? Pass the rest forward recursively to run in parallel
    if elems.len() > 0 {
        rayon::join(
            || {
                let next_row_indexes =
                    build_next_row(tree_mutex, preimage_mutex, elems, update_proofs, row_depth);
                let mut next_proofs = next_row.lock();
                for (i, prev_res) in next_row_indexes {
                    next_proofs.insert(i, prev_res);
                }
                drop(next_proofs);
            },
            || {
                split_and_run_next_row_inner(
                    tree_mutex,
                    preimage_mutex,
                    update_proofs,
                    next_row,
                    row_depth,
                    n + 1,
                )
            },
        );
    }
}

// ------------------------------

fn build_first_row(
    tree_mutex: &Arc<Mutex<&mut Tree>>,
    preimage_mutex: &Arc<Mutex<&mut Map<String, Value>>>,
    entries: Vec<(&u64, &BigUint)>, // 4 entries taken from the hashmap to be updated in parallel
    hashes: &HashMap<u64, BigUint>, // the whole hashmap
) -> Vec<(u64, BigUint)> {
    // next row stores the indexes of the next row that need to be updated
    // (and the previous result hashes for the init state preimage)
    let mut next_row: Vec<(u64, BigUint)> = Vec::new();

    for (idx, hash) in entries.iter() {
        // ! Left child
        if *idx % 2 == 0 {
            //? If the right child exists, hash them together in the next loop
            if hashes.get(&(*idx + 1)).is_some() {
                continue;
            }
            //? If the right child doesn't exist (wasn't updated), hash the left child with the previous value in the state tree
            else {
                // ? Get the previous values in the state tree
                let tree = tree_mutex.lock();
                let init_left_hash = tree.nth_leaf_node(**idx);
                let right_hash = &tree.nth_leaf_node(*idx + 1);
                drop(tree);

                // ? Hash the left child with the right child
                let new_hash = pedersen(&hash, &right_hash);

                // ? Use the new_hash to update the merkle tree
                let mut tree = tree_mutex.lock();
                let prev_res_hash = tree.ith_inner_node(1, *idx / 2);
                tree.update_inner_node(1, *idx / 2, new_hash.clone());
                drop(tree);

                next_row.push((*idx / 2, prev_res_hash.clone()));

                // * Preimages -----------------------------------------------------------------------------------------------

                // ? Insert the new hash info into the preimage
                let mut preimage = preimage_mutex.lock();

                if !preimage.contains_key(&prev_res_hash.to_string()) {
                    preimage.insert(
                        prev_res_hash.to_string(),
                        serde_json::to_value([init_left_hash.to_string(), right_hash.to_string()])
                            .unwrap(),
                    );
                }

                preimage.insert(
                    new_hash.to_string(),
                    serde_json::to_value([hash.to_string(), right_hash.to_string()]).unwrap(),
                );
                drop(preimage);

                // * Preimages -----------------------------------------------------------------------------------------------

                // ? update the leaf node with the hash
                let mut tree = tree_mutex.lock();
                tree.update_leaf_node(hash, **idx);
                drop(tree);
            }
        }
        // ! Right child
        else {
            // ? get the left child hash
            let left_hash: BigUint;
            let prev_left_hash: BigUint;
            let prev_right_hash: BigUint;
            if hashes.get(&(*idx - 1)).is_some() {
                // ? If the left child exists, hash them together
                left_hash = hashes.get(&(*idx - 1)).unwrap().clone();
                let mut tree = tree_mutex.lock();
                prev_left_hash = tree.nth_leaf_node(*idx - 1);
                prev_right_hash = tree.nth_leaf_node(**idx);

                // ? Update the nodes in the tree with the hashes
                tree.update_leaf_node(&left_hash, **idx - 1);
                tree.update_leaf_node(hash, **idx);

                drop(tree);
            } else {
                //? If the left child doesn't exist, hash the right child with the previous value in the state tree
                let mut tree = tree_mutex.lock();
                left_hash = tree.nth_leaf_node(*idx - 1);
                prev_left_hash = tree.nth_leaf_node(*idx - 1);
                prev_right_hash = tree.nth_leaf_node(**idx);

                // ? Update the nodes in the tree with the hashes
                tree.update_leaf_node(hash, **idx);
                drop(tree);
            };

            // ? Hash the left child with the right child
            let new_hash = pedersen(&left_hash, &hash);

            // ? Use the new_hash to update the merkle tree
            let mut tree = tree_mutex.lock();
            let prev_res_hash = tree.ith_inner_node(1, *idx / 2);
            tree.update_inner_node(1, *idx / 2, new_hash.clone());
            drop(tree);
            next_row.push((*idx / 2, prev_res_hash.clone()));

            // * Preimages -----------------------------------------------------------------------------------------------

            // ? Insert the new hash info into the preimage
            let mut preimage = preimage_mutex.lock();

            if !preimage.contains_key(&prev_res_hash.to_string()) {
                preimage.insert(
                    prev_res_hash.to_string(),
                    serde_json::to_value([prev_left_hash.to_string(), prev_right_hash.to_string()])
                        .unwrap(),
                );
            }

            preimage.insert(
                new_hash.to_string(),
                serde_json::to_value([left_hash.to_string(), hash.to_string()]).unwrap(),
            );
            drop(preimage);

            // * Preimages -----------------------------------------------------------------------------------------------
        }
    }

    return next_row;
}

fn build_next_row(
    tree_mutex: &Arc<Mutex<&mut Tree>>,
    preimage_mutex: &Arc<Mutex<&mut Map<String, Value>>>,
    entries: Vec<(&u64, &BigUint)>, // 4 entries taken from the hashmap to be updated in parallel
    hashes: &HashMap<u64, BigUint>, // the whole hashmap
    row_depth: usize,
) -> Vec<(u64, BigUint)> {
    // next row stores the indexes of the next row that need to be updated
    // (and the previous result hashes for the init state preimage)
    let mut next_row: Vec<(u64, BigUint)> = Vec::new();

    for (idx, prev_res) in entries.iter() {
        // ! Left child
        if *idx % 2 == 0 {
            //? If the right child exists, hash them together in the next loop
            if hashes.get(&(*idx + 1)).is_some() {
                continue;
            }
            //? If the right child doesn't exist (hasn't been updated), hash the left child with the previous value in the state tree
            else {
                // ? Get the previous values from the state tree
                let tree = tree_mutex.lock();
                let hash = &tree.ith_inner_node(row_depth as u32, **idx);
                let right_hash = &tree.ith_inner_node(row_depth as u32, *idx + 1);
                drop(tree);

                // ? Hash the left child with the right child
                let new_hash = pedersen(hash, right_hash);

                // ? Use the new_hash to update the merkle tree
                let mut tree = tree_mutex.lock();
                let prev_res_hash = tree.ith_inner_node(row_depth as u32 + 1, *idx / 2);
                tree.update_inner_node(row_depth as u32 + 1, *idx / 2, new_hash.clone());
                drop(tree);
                next_row.push((*idx / 2, prev_res_hash.clone()));

                // * Preimages -----------------------------------------------------------------------------------------------

                // ? Insert the new hash info into the preimage
                let mut preimage = preimage_mutex.lock();

                // ? Previous batch state preimage
                if !preimage.contains_key(&prev_res_hash.to_string()) {
                    preimage.insert(
                        prev_res_hash.to_string(),
                        serde_json::to_value([prev_res.to_string(), right_hash.to_string()])
                            .unwrap(),
                    );
                }

                // ? Current batch state preimage
                preimage.insert(
                    new_hash.to_string(),
                    serde_json::to_value([hash.to_string(), right_hash.to_string()]).unwrap(),
                );
                drop(preimage);

                // * Preimages -----------------------------------------------------------------------------------------------
            }
        }
        // ! Right child
        else {
            // ? Get the left and right hashes
            let tree = tree_mutex.lock();

            let hash = &tree.ith_inner_node(row_depth as u32, **idx);
            let left_hash = &tree.ith_inner_node(row_depth as u32, *idx - 1);
            let prev_left_hash: BigUint;
            if let Some(prev_left) = hashes.get(&(*idx - 1)) {
                prev_left_hash = prev_left.clone();
            } else {
                prev_left_hash = left_hash.clone();
            }
            let prev_right_hash = *prev_res;

            drop(tree);

            // ? Hash the left child with the right child
            let new_hash = pedersen(&left_hash, &hash);

            // ? Use the new_hash to update the merkle tree
            let mut tree = tree_mutex.lock();
            let prev_res_hash = tree.ith_inner_node(row_depth as u32 + 1, *idx / 2);
            tree.update_inner_node(row_depth as u32 + 1, *idx / 2, new_hash.clone());
            drop(tree);

            next_row.push((*idx / 2, prev_res_hash.clone()));

            // * Preimages -----------------------------------------------------------------------------------------------

            // ? Insert the new hash info into the preimage
            let mut preimage = preimage_mutex.lock();

            // ? Previous batch state preimage
            if !preimage.contains_key(&prev_res_hash.to_string()) {
                preimage.insert(
                    prev_res_hash.to_string(),
                    serde_json::to_value([prev_left_hash.to_string(), prev_right_hash.to_string()])
                        .unwrap(),
                );
            }

            // ? Current batch state preimage
            preimage.insert(
                new_hash.to_string(),
                serde_json::to_value([left_hash.to_string(), hash.to_string()]).unwrap(),
            );
            drop(preimage);

            // * Preimages -----------------------------------------------------------------------------------------------
        }
    }

    return next_row;
}

pub fn build_tree(depth: u32, leaf_nodes: &Vec<BigUint>, shift: u32) -> BigUint {
    let inner_nodes: Vec<Vec<BigUint>> = inner_from_leaf_nodes2(depth as usize, leaf_nodes, shift);
    let root = inner_nodes[0][0].clone();

    return root;
}

fn inner_from_leaf_nodes2(
    depth: usize,
    leaf_nodes: &Vec<BigUint>,
    shift: u32,
) -> Vec<Vec<BigUint>> {
    let mut tree: Vec<Vec<BigUint>> = Vec::new();

    let first_row = leaf_nodes;

    let len = leaf_nodes.len();
    let new_len = if len % 2 == 0 { len / 2 } else { len / 2 + 1 };
    let mut hashes: Vec<BigUint> = vec![BigUint::zero(); new_len];
    let hashes_mutex = Arc::new(Mutex::new(&mut hashes));
    hash_tree_level(&hashes_mutex, &first_row, 0, 0, shift);
    tree.push(hashes);

    for i in 1..depth {
        let len = &tree[i - 1].len();
        let new_len = if len % 2 == 0 { len / 2 } else { len / 2 + 1 };
        let mut hashes: Vec<BigUint> = vec![BigUint::zero(); new_len];
        let hashes_mutex = Arc::new(Mutex::new(&mut hashes));
        hash_tree_level(&hashes_mutex, &tree[i - 1], i, 0, shift);
        tree.push(hashes);
    }

    tree.reverse();
    return tree;
}

fn hash_tree_level(
    next_row: &Arc<Mutex<&mut Vec<BigUint>>>,
    leaf_nodes: &Vec<BigUint>,
    i: usize,
    n: usize,
    shift: u32,
) {
    let inp_array = leaf_nodes
        .iter()
        .skip(n * STRIDE)
        .take(STRIDE)
        .collect::<Vec<&BigUint>>();

    if inp_array.len() > 0 {
        rayon::join(
            || {
                let next_row_hashes = pairwise_hash2(&inp_array, i, shift);
                let mut next_hashes = next_row.lock();

                let hashes_len = next_hashes.len();
                if hashes_len < (n * STRIDE) / 2 + STRIDE / 2 || next_row_hashes.len() < STRIDE / 2
                {
                    next_hashes.as_mut_slice()[(n * STRIDE) / 2..]
                        .clone_from_slice(&next_row_hashes);
                } else {
                    next_hashes.as_mut_slice()[(n * STRIDE) / 2..(n * STRIDE) / 2 + STRIDE / 2]
                        .clone_from_slice(&next_row_hashes);
                }

                drop(next_hashes);
            },
            || hash_tree_level(next_row, leaf_nodes, i, n + 1, shift),
        );
    }
}

pub fn pairwise_hash2(array: &Vec<&BigUint>, i: usize, shift: u32) -> Vec<BigUint> {
    // This should be an array of STRIDE length

    let mut hashes: Vec<BigUint> = Vec::new();
    for j in (0..array.len() - 1).step_by(2) {
        let hash = pedersen(&array[j], &array[j + 1]);
        hashes.push(hash);
    }

    if array.len() % 2 == 1 {
        hashes.push(pedersen(
            &array[array.len() - 1],
            &get_zero_hash(i as u32, shift),
        ));
    }

    return hashes;
}
