use crate::block::{ProposerBlock, TXBlock};
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

    NewTransactionBlockHashes(Vec<H256>),
    GetTransactionBlocks(Vec<H256>),
    TransactionBlocks(Vec<TXBlock>),

    NewProposerBlockHashes(Vec<H256>),
    GetProposerBlocks(Vec<H256>),
    ProposerBlocks(Vec<ProposerBlock>),
}
