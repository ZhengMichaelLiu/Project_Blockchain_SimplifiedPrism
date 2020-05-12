use crate::crypto::hash::{H256, Hashable};
use crate::transaction::SignedTransaction;
use serde::{Serialize, Deserialize};

// Header for block
#[derive(Serialize, Deserialize, Debug,Clone,Copy)]
pub struct Header {
    pub parent : H256,
    pub nonce : u32,
    pub timestamp : u128,
    pub difficulty : H256,
    pub merkle : H256,
}

impl Hashable for Header {
    fn hash(&self) -> H256 {
        let serialized = serde_json::to_string(self).unwrap();
        ring::digest::digest(&ring::digest::SHA256, serialized.as_bytes()).into()
    }
}

#[derive(Serialize, Deserialize, Debug,Clone)]
pub struct Content {
    pub transaction_content: Vec<SignedTransaction>,
}

// Definition of block
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Block {
    pub header : Header,
    pub content : Content,
}

impl Hashable for Block {
    fn hash(&self) -> H256 {
        self.header.hash()
    }
}

#[cfg(any(test, test_utilities))]
pub mod test {
    use super::*;
    use crate::crypto::merkle::MerkleTree;
    use std::time::SystemTime;

    pub fn generate_random_block(parent: &H256) -> Block {
        let fake_content : Vec::<SignedTransaction> = Vec::new();
        let fake_root : H256 = MerkleTree::new(&fake_content).root();

        let header = Header{
            parent: *parent,
            nonce: 1,
            difficulty: H256([0;32]),
            timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis(),
            merkle: fake_root,
        };

        let content = Content{
            transaction_content: fake_content,
        };

        return Block{
            header : header,
            content : content,
        }
    }
}
