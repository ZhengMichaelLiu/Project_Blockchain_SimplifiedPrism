use crate::block::Block;
use crate::crypto::hash::{H256,H160};
use crate::transaction::SignedTransaction;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Message {
    
    NewAddr(Vec<H160>),
    ICO(H256, HashMap<(H256, u32), (u32, H160)>),

    NewTransactionHashes(Vec<H256>),
    GetTransactions(Vec<H256>),
    Transactions(Vec<SignedTransaction>),

    NewBlockHashes(Vec<H256>),
    GetBlocks(Vec<H256>),
    Blocks(Vec<Block>),
}
