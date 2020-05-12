use crate::block::{Header, Content, Block};
use crate::transaction::SignedTransaction;
use crate::crypto::merkle::MerkleTree;
use crate::crypto::hash::{H256, Hashable};

use std::collections::HashMap;

pub struct Blockchain {
    // hash of blocks and blocks themselves
    pub blocks : HashMap<H256, Block>, 
    
    // hash of blocks and their heights
    pub heights : HashMap<H256, u32>, 
    
    // height of tip
    pub longest : u32,

    // the tip node
	pub tip_node_hash : H256 
}

impl Blockchain {
    /// Create a new blockchain, only containing the genesis block
    pub fn new() -> Self {
        let difficulty : [u8; 32] = [7, 255, 255, 255, 255, 255, 255, 255, 
            255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255,];

        let fake_content : Vec<SignedTransaction> = Vec::new();

        let header : Header = Header {
            parent: H256([0;32]),
            nonce: 1,
            timestamp: 1,
            difficulty: <H256>::from(&difficulty),
            merkle: MerkleTree::new(&fake_content).root(),
        };

        let content : Content = Content {
            transaction_content: fake_content,
        };      

        let genesis_block : Block = Block {
            header: header,   
            content: content,
        };

        let mut blocks_hash_map = HashMap::new();
        blocks_hash_map.insert(genesis_block.clone().hash(), genesis_block.clone());
    
        let mut height_hash_map = HashMap::new();
        height_hash_map.insert(genesis_block.clone().hash(), 0);
    
        return Blockchain {
            blocks: blocks_hash_map,
            heights: height_hash_map,
            longest: 0,
            tip_node_hash: genesis_block.clone().hash(),
        };    
    }

    /// Insert a block into blockchain
    pub fn insert(&mut self, block: &Block) {
        self.blocks.insert(block.clone().hash(), block.clone());
		let curr_height = self.heights[&block.clone().header.parent] + 1;
		self.heights.insert(block.clone().hash(), curr_height);
		if curr_height > self.longest {
			self.longest = curr_height;
			self.tip_node_hash = block.clone().hash();
		}
    }

    /// Get the last block's hash of the longest chain
    pub fn tip(&self) -> H256 {
        self.tip_node_hash
    }

    pub fn all_blocks_in_longest_chain(&self) -> Vec<H256> {
        let mut result = Vec::new();
        let mut curr_node = self.tip_node_hash;
    
        if self.longest == 0 {
            result.push(self.tip_node_hash);
            return result;
        }
    
        for _i in 0..=self.longest {
            result.push(curr_node);
            curr_node = self.blocks[&curr_node].header.parent;
        }
    
        result.reverse();
        return result;
    }
}

#[cfg(any(test, test_utilities))]
mod tests {
    use super::*;
    use crate::block::test::generate_random_block;
    use crate::crypto::hash::Hashable;

    #[test]
    fn insert_one() {
        let mut blockchain = Blockchain::new();
        let genesis_hash = blockchain.tip();
        let block = generate_random_block(&genesis_hash);
        blockchain.insert(&block);
        assert_eq!(blockchain.tip(), block.hash());
    }
}
