use crate::crypto::hash::{H256, Hashable};
use crate::transaction::SignedTransaction;
use serde::{Serialize, Deserialize};

// Header for super block
#[derive(Serialize, Deserialize, Debug,Clone,Copy)]
pub struct Header {
    pub proposer_parent : H256,
    pub nonce : u32,
    pub timestamp : u128,
    pub transaction_difficulty : H256,
    pub proposer_difficulty : H256,
    pub merkle_transaction : H256,
    pub merkle_proposer : H256,
}

impl Hashable for Header {
    fn hash(&self) -> H256 {
        let serialized = serde_json::to_string(self).unwrap();
        ring::digest::digest(&ring::digest::SHA256, serialized.as_bytes()).into()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProposerContent {
    pub proposer_content : Vec<H256>,
}

// Definition of super block
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProposerBlock {
    pub header : Header,
    pub content : ProposerContent,
}

impl Hashable for ProposerBlock {
    fn hash(&self) -> H256 {
        self.header.hash()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TXContent {
    pub transaction_content : Vec<SignedTransaction>,
}

// Definition of block
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TXBlock {
    pub header : Header,
    pub content : TXContent,
}

impl Hashable for TXBlock {
    fn hash(&self) -> H256 {
        self.header.hash()
    }
}

#[cfg(any(test, test_utilities))]
pub mod test {
    use super::*;
    use crate::crypto::hash::H256;

    pub fn generate_random_block(parent: &H256) -> ProposerBlock {
        let header = Header {
            proposer_parent: *parent,
            nonce: random::<u32>(),
            timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis(),
            transaction_difficulty: H256([0;32]),
            proposer_difficulty: H256([0;32]),
            merkle_transaction: H256([0;32]),
            merkle_proposer: H256([0;32]),
        };

        let content = ProposerContent {
            proposer_content: Vec::<H256>::new(),
        };

        return ProposerBlock {
            header : header,
            content : content,
        }
    }
}
