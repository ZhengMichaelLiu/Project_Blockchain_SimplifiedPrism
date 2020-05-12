use super::hash::{Hashable, H256};
use ring;
/// A Merkle tree.
#[derive(Debug, Default)]
pub struct MerkleTree {
    pub all_nodes : Vec<H256>,
	pub mod_height : usize,
}

impl MerkleTree {
    pub fn new<T>(data: &[T]) -> Self where T: Hashable, {
        let mut all_nodes = Vec::new();
        if data.len() == 0{
            return MerkleTree{
                all_nodes : all_nodes,
                mod_height : 0,
            };
        }
        let mod_height = (data.len() as f64).log(2.0).ceil() as u32;
        let dest = 2u32.pow(mod_height);
        // println!("{}",dest);
        for i in 0..(dest as usize){
            if i < data.len(){
                all_nodes.push(data[i].hash());
            }
            else{
                all_nodes.push(data[data.len()-1].hash());
            }
        }
        let mut offset : usize = 0;
        let mut next : usize;
        for k in 0..(mod_height as usize){
            // println!("{}",next);
            next = all_nodes.len() as usize;
            let range = 2u32.pow(mod_height-1-(k as u32));
            for i in 0..(range as usize){
                let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
                ctx.update(&(<[u8; 32]>::from(all_nodes[offset+i*2])));
                ctx.update(&(<[u8; 32]>::from(all_nodes[offset+i*2+1])));
                let comb_hash = ctx.finish();
                let hash_conca = H256::from(comb_hash);
                all_nodes.push(hash_conca);
            }
            offset = next;
        }
        // for i in 0..all_nodes.len(){
        //     println!("{:?}",all_nodes[i]);
        // }
        return MerkleTree{
            all_nodes : all_nodes,
            mod_height : mod_height as usize,
        };
    }

    pub fn root(&self) -> H256 {
        if self.all_nodes.len() == 0{
            return H256([0;32]);
        }
        return self.all_nodes[self.all_nodes.len()-1];
    }

    /// Returns the Merkle Proof of data at index i
    pub fn proof(&self, index: usize) -> Vec<H256> {
        let mut ret = Vec::new();
        let mut index :u32= index as u32;
        let mut offset = 0;
        for i in 0..(self.mod_height){
            // println!("{} {}",index,offset);
            if index %2 != 0{
                ret.push(self.all_nodes[(offset+index-1) as usize]);
            }
            else{
                ret.push(self.all_nodes[(offset+index+1) as usize]);
            }
            index = index/2;
            offset += 2u32.pow((self.mod_height-i) as u32);
        }
        // for i in 0..ret.len(){
        //     println!("\n{:?}",ret[i]);
        // }
        return ret;
    }
}

/// Verify that the datum hash with a vector of proofs will produce the Merkle root. Also need the
/// index of datum and `leaf_size`, the total number of leaves.
pub fn verify(root: &H256, datum: &H256, proof: &[H256], index: usize, _leaf_size: usize) -> bool {
    let mut cur = *datum;
    let mut idx = index;
    for i in 0..proof.len(){
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
        if idx %2 == 0{
            ctx.update(&<[u8; 32]>::from(cur));
            ctx.update(&(<[u8; 32]>::from(proof[i])));
        }
        else{
            ctx.update(&(<[u8; 32]>::from(proof[i])));
            ctx.update(&<[u8; 32]>::from(cur));
        }
        idx /= 2;
        let comb_hash = ctx.finish();
        cur = H256::from(comb_hash);
    }
    // println!("{:?}  {:?}",*root,cur);
    return *root == cur;
}

#[cfg(test)]
mod tests {
    use crate::crypto::hash::H256;
    use super::*;

    macro_rules! gen_merkle_tree_data {
        () => {{
            vec![
                (hex!("0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d")).into(),
                (hex!("0101010101010101010101010101010101010101010101010101010101010202")).into(),
	
		(hex!("0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d")).into(),
                (hex!("0101010101010101010101010101010101010101010101010101010101010202")).into(),
            ]
        }};
    }

    #[test]
    fn root() {
        let input_data: Vec<H256> = gen_merkle_tree_data!();
        let merkle_tree = MerkleTree::new(&input_data);
        let root = merkle_tree.root();
        assert_eq!(
            root,
            (hex!("a4efcefb722cea4fc3a4b863e1d7ad7025e726d79ff1c74b5f745ad1dca874ca")).into()
        );
        // "b69566be6e1720872f73651d1851a0eae0060a132cf0f64a0ffaea248de6cba0" is the hash of
        // "0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d0a0b0c0d0e0f0e0d"
        // "965b093a75a75895a351786dd7a188515173f6928a8af8c9baa4dcff268a4f0f" is the hash of
        // "0101010101010101010101010101010101010101010101010101010101010202"
        // "6b787718210e0b3b608814e04e61fde06d0df794319a12162f287412df3ec920" is the hash of
        // the concatenation of these two hashes "b69..." and "965..."
        // notice that the order of these two matters
    }

    #[test]
    fn proof() {
        let input_data: Vec<H256> = gen_merkle_tree_data!();
        let merkle_tree = MerkleTree::new(&input_data);
        let proof = merkle_tree.proof(2);
        assert_eq!(proof,
                   vec![hex!("965b093a75a75895a351786dd7a188515173f6928a8af8c9baa4dcff268a4f0f").into(),
			hex!("6b787718210e0b3b608814e04e61fde06d0df794319a12162f287412df3ec920").into()
			]
        );
        // "965b093a75a75895a351786dd7a188515173f6928a8af8c9baa4dcff268a4f0f" is the hash of
        // "0101010101010101010101010101010101010101010101010101010101010202"
    }

    #[test]
    fn verifying() {
        let input_data: Vec<H256> = gen_merkle_tree_data!();
        let merkle_tree = MerkleTree::new(&input_data);
        let proof = merkle_tree.proof(3);
        assert!(verify(&merkle_tree.root(), &input_data[3].hash(), &proof, 3, input_data.len()));
    }
}
