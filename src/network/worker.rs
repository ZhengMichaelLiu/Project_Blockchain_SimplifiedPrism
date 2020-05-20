use crate::block::{ProposerBlock, TXBlock};
use crate::blockchain::Blockchain;
use crate::crypto::hash::{H256,H160,Hashable};
use super::message::Message;
use crate::network::server::Handle as ServerHandle;
use super::peer;
use crate::transaction::{SignedTransaction,verify};

use crossbeam::channel;
use log::{debug, warn};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::thread;
use ring::signature::Ed25519KeyPair;

#[derive(Clone)]
pub struct Context {
    msg_chan: channel::Receiver<(Vec<u8>, peer::Handle)>,
    num_worker: usize,
    server: ServerHandle,
    blockchain: Arc<Mutex<Blockchain>>,
    orphan_handler: Arc<Mutex<HashMap<H256, Vec<ProposerBlock>>>>,
    tx_pool: Arc<Mutex<HashMap<H256, SignedTransaction>>>,
    glob_state: Arc<Mutex<HashMap<H256, HashMap<(H256, u32), (u32, H160)>>>>,
    all_addr: Arc<Mutex<Vec<H160>>>,
    my_addr_key: Arc<Mutex<HashMap<H160, Ed25519KeyPair>>>,
    tx_block_pool: Arc<Mutex<HashMap<H256, TXBlock>>>,
    tx_block_ref: Arc<Mutex<HashMap<H256, Vec<H256>>>>,
}

pub fn new(
    num_worker: usize,
    msg_src: channel::Receiver<(Vec<u8>, peer::Handle)>,
    server: &ServerHandle,
    blockchain: &Arc<Mutex<Blockchain>>,
    orphan_handler: &Arc<Mutex<HashMap<H256, Vec<ProposerBlock>>>>,
    tx_pool: &Arc<Mutex<HashMap<H256, SignedTransaction>>>,
    glob_state: &Arc<Mutex<HashMap<H256, HashMap<(H256, u32), (u32, H160)>>>>,
    all_addr: &Arc<Mutex<Vec<H160>>>,
    my_addr_key: &Arc<Mutex<HashMap<H160, Ed25519KeyPair>>>,
    tx_block_pool: &Arc<Mutex<HashMap<H256, TXBlock>>>,
    tx_block_ref: &Arc<Mutex<HashMap<H256, Vec<H256>>>>,
) -> Context {
    Context {
        msg_chan: msg_src,
        num_worker,
        server: server.clone(),
        blockchain: Arc::clone(blockchain),
        orphan_handler: Arc::clone(orphan_handler),
        tx_pool: Arc::clone(tx_pool),
        glob_state: Arc::clone(glob_state),
        all_addr: Arc::clone(all_addr),
        my_addr_key: Arc::clone(my_addr_key),
        tx_block_pool: Arc::clone(tx_block_pool),
        tx_block_ref: Arc::clone(tx_block_ref),
    }
}

impl Context {
    pub fn start(self) {
        let num_worker = self.num_worker;
        for i in 0..num_worker {
            let cloned = self.clone();
            thread::spawn(move || {
                cloned.worker_loop();
                warn!("Worker thread {} exited", i);
            });
        }
    }

    // this function is used to handle the orphan blocks
    // when a new block arrives, we need to check if the new block is a missing parent to some orphans
    fn new_parent(&self, block_item : &ProposerBlock, peer : &peer::Handle){
        // check if it is a missing parent
        if self.orphan_handler.lock().unwrap().contains_key(&block_item.hash()){
            // if it is, then get its waiting children
            let vec = self.orphan_handler.lock().unwrap()[&block_item.hash()].clone();

            // remove the orphans
            self.orphan_handler.lock().unwrap().remove(&block_item.hash());

            // process each orphan
            for each_proposer_block in &vec {
                // When receiving and processing a block,
                // check if the transaction is signed correctly by the public key(s).
                let mut valid = true;
                for each_ref in each_proposer_block.content.proposer_content.clone() {
                    let curr_tx_block = self.tx_block_pool.lock().unwrap()[&each_ref].clone();
                    for each_tx in curr_tx_block.content.transaction_content.iter() {
                        if verify(&each_tx.transaction, &each_tx.sig.key, &each_tx.sig.signature) == false {
                            debug!("Transaction signed wrong");
                            valid = false;
                            break;
                        }        
                    }
                    if valid == false {
                        break;
                    }
                }
                if valid == false {
                    continue;
                }
                // check the public key(s) matches the owner(s)'s address of these inputs.
                // check if the inputs to the transactions are not spent
                // check the values of inputs are not less than those of outputs.

                // update tx_block_ref
                let mut new_ref : Vec<H256> = self.tx_block_ref.lock().unwrap()[&each_proposer_block.header.proposer_parent].clone();
                let my_ref : Vec<H256> = each_proposer_block.content.proposer_content.clone();
                for each_ref in my_ref.iter() {
                    if new_ref.contains(&each_ref) == false {
                        new_ref.push(*each_ref);
                    }
                }
                self.tx_block_ref.lock().unwrap().insert(each_proposer_block.hash(), new_ref.clone());

                // update state ledger
                let mut curr_state = self.glob_state.lock().unwrap()[&each_proposer_block.header.proposer_parent].clone();
                for each_ref in my_ref.iter() {
                    let curr_tx_block = self.tx_block_pool.lock().unwrap()[&each_ref].clone();
                    for each_tx in curr_tx_block.content.transaction_content.iter() {
                        let mut lookup_key = (H256([0;32]),0); // initialized to be a fake look up key
                        let mut exist = false;
                        for i in 0..each_tx.transaction.input_previous.len() {
                            lookup_key = (each_tx.transaction.input_previous[i], each_tx.transaction.input_index[i]);
                            // if this transaction is valid, then update the ledger // else not update the ledger
                            if curr_state.contains_key(&lookup_key) {
                                exist = true;
                                curr_state.insert((each_tx.hash(), i as u32), (each_tx.transaction.output_value[i], each_tx.transaction.output_address[i]));
                            }
                        }
                        if exist == true {
                            curr_state.remove(&lookup_key);
                        }
                    }
                }

                // update transaction pool, remove the transactions that are conflict with the new state.
                let mut curr_tx_pool = self.tx_pool.lock().unwrap();
                let mut to_remove_tx : Vec<H256> = Vec::new();
                for each_tx in curr_tx_pool.keys() {
                    for idx in 0..curr_tx_pool[&each_tx].transaction.input_previous.len() {
                        let lookup_key = (curr_tx_pool[&each_tx].transaction.input_previous[idx],
                                        curr_tx_pool[&each_tx].transaction.input_index[idx]);
                        if curr_state.contains_key(&lookup_key) == false {
                            to_remove_tx.push(*each_tx);
                        }
                    }
                }
                for key in to_remove_tx.iter() {
                    // debug!("Removing old transactions");
                    curr_tx_pool.remove(key);
                }
                std::mem::drop(curr_tx_pool);

                // insert new state
                self.glob_state.lock().unwrap().insert(each_proposer_block.hash(), curr_state.clone());

                // insert new block
                self.blockchain.lock().unwrap().insert(&each_proposer_block.clone());

                // Relay blocks
                self.server.broadcast(Message::NewProposerBlockHashes(vec![each_proposer_block.hash()]));

                // check new parent
                self.new_parent(&each_proposer_block, &peer);
            }
        }
    }

    fn worker_loop(&self) {
        loop {
            let msg = self.msg_chan.recv().unwrap();
            let (msg, peer) = msg;
            let msg : Message = bincode::deserialize(&msg).unwrap();
            match msg {

                // When a node is started, it would tell other nodes its addresses
                Message::NewAddr(addr_vec) => {
                    for each_addr in addr_vec {
                        if self.all_addr.lock().unwrap().contains(&each_addr) == false {
                            self.all_addr.lock().unwrap().push(each_addr);
                            self.server.broadcast(Message::NewAddr(self.all_addr.lock().unwrap().to_vec()));
                        }
                    }
                }

                // Initial Coin Offering propagating on the network
                Message::ICO(genesis_hash, ico_state) => {
                    if self.glob_state.lock().unwrap().is_empty() == true {
                        self.glob_state.lock().unwrap().insert(genesis_hash, ico_state.clone());
                        self.server.broadcast(Message::ICO(genesis_hash, ico_state.clone()));
                    }
                }
                
                Message::NewTransactionHashes(transaction_hash_vec) => {
                    // if no this transaction, then ask for it
                    for each_tx_hash in transaction_hash_vec {
                        if self.tx_pool.lock().unwrap().contains_key(&each_tx_hash) == false {
                            peer.write(Message::GetTransactions(vec![each_tx_hash]));
                        }
                    }
                }

                Message::GetTransactions(transaction_hash_vec) => {
                    // if transaction in the transaction pool, then send it out to peer
                    for each_tx_hash in transaction_hash_vec {
                        if self.tx_pool.lock().unwrap().contains_key(&each_tx_hash) == true {
                            peer.write(Message::Transactions(vec![self.tx_pool.lock().unwrap()[&each_tx_hash].clone()]));
                        }
                    }
                }

                Message::Transactions(transaction_vec) => {
                    let tip = self.blockchain.lock().unwrap().tip();

                    for each_tx in transaction_vec {
                        // check if this transaction is in the transaction pool
                        if self.tx_pool.lock().unwrap().contains_key(&each_tx.hash()) == true {
                            debug!("Transaction already in pool");
                            continue;
                        }

                        // check if the transaction is signed correctly by the public key(s).
                        if verify(&each_tx.transaction, &each_tx.sig.key, &each_tx.sig.signature) == false {
                            debug!("Transaction signed wrong");
                            continue;
                        }

                        let curr_state = self.glob_state.lock().unwrap()[&tip].clone();
                        // check if the inputs to the transactions are not spent
                        let mut valid = true;
                        for i in 0..each_tx.transaction.input_previous.len() {
                            if curr_state.contains_key(&(each_tx.transaction.input_previous[i], each_tx.transaction.input_index[i])) == false {
                                valid=false;
                                break;
                            }
                        }
                        if valid == false {
                            // debug!("Transaction spent");
                            continue;
                        }

                        // check the public key(s) matches the owner(s)'s address of these inputs.
                        valid = true;
                        for i in 0..each_tx.transaction.input_previous.len() {
                            let (_value, addr) = curr_state[&(each_tx.transaction.input_previous[i], each_tx.transaction.input_index[i])];
                            let h:H256 = ring::digest::digest(&ring::digest::SHA256, &(each_tx.sig.key[..])).into();
                            let pub_key = H160::from(h);
                            if addr != pub_key {
                                valid = false;
                                break;
                            }
                        }
                        if valid == false {
                            debug!("Transaction key address not match");
                            continue;
                        }
                        
                        let mut input_sum : u32 = 0;
                        let mut output_sum : u32 = 0;
                        // check the values of inputs are not less than those of outputs.
                        for i in 0..each_tx.transaction.input_previous.len() {
                            let (value, _addr) = curr_state[&(each_tx.transaction.input_previous[i], each_tx.transaction.input_index[i])];
                            input_sum = value;
                            output_sum += each_tx.transaction.output_value[i];
                        }
                        if output_sum > input_sum {
                            debug!("Transaction output larger");
                            continue;
                        }

                        //  If a new transaction passes the above checks, add it to the transaction pool. And broadcast
                        self.tx_pool.lock().unwrap().insert(each_tx.hash(), each_tx.clone());
                        self.server.broadcast(Message::NewTransactionHashes(vec![each_tx.hash()]));
                    }
                }

                Message::NewTransactionBlockHashes(tx_block_hash_vec) => {
                    for each_tx_block_hash in tx_block_hash_vec {
                        if self.tx_block_pool.lock().unwrap().contains_key(&each_tx_block_hash) == false {
                            peer.write(Message::GetTransactionBlocks(vec![each_tx_block_hash]));
                        }
                    }
                }

                Message::GetTransactionBlocks(tx_block_hash_vec) => {
                    for each_tx_block_hash in tx_block_hash_vec {
                        if self.tx_block_pool.lock().unwrap().contains_key(&each_tx_block_hash) == true {
                            peer.write(Message::TransactionBlocks(vec![self.tx_block_pool.lock().unwrap()[&each_tx_block_hash].clone()]));
                        }
                    }
                }

                Message::TransactionBlocks(tx_block_vec) => {
                    for each_tx_block in tx_block_vec {
                        // if this transaction block is not in the pool
                        if self.tx_block_pool.lock().unwrap().contains_key(&each_tx_block.hash()) == true {
                            debug!("Transaction Block in the pool");
                            continue;
                        }

                        // PoW check: check if tx_block.hash > proposer_difficulty && tx_block.hash < transction_difficulty 
                        if each_tx_block.hash() > each_tx_block.header.transaction_difficulty {
                            debug!("Transaction block wrong sortition");
                            continue;
                        }
                        if each_tx_block.hash() <= each_tx_block.header.proposer_difficulty {
                            debug!("Transaction block wrong sortition");
                            continue;
                        }

                        // check if this transaction block is empty
                        // commented out for attack
                        /*
                        if each_tx_block.content.transaction_content.len() == 0 {
                            debug!("Empty Transaction Block");
                            continue;
                        }
                        */

                        // remove the transaction contained in this block from the transaction pool
                        for transaction in each_tx_block.content.transaction_content.clone() {
                            if self.tx_pool.lock().unwrap().contains_key(&transaction.hash()) == false {
                                continue;
                            }
                            self.tx_pool.lock().unwrap().remove(&transaction.hash());   
                        }

                        // insert this block into the transaction block pool
                        self.tx_block_pool.lock().unwrap().insert(each_tx_block.hash(), each_tx_block.clone());

                        // Relay blocks
                        self.server.broadcast(Message::NewTransactionBlockHashes(vec![each_tx_block.hash()]));
                    }
                }

                Message::NewProposerBlockHashes(proposer_block_hash_vec) => {
                    for each_proposer_block_hash in proposer_block_hash_vec {
                        if self.blockchain.lock().unwrap().blocks.contains_key(&each_proposer_block_hash) == false {
                            peer.write(Message::GetProposerBlocks(vec![each_proposer_block_hash]));
                        }
                    }
                }

                Message::GetProposerBlocks(proposer_block_hash_vec) => {
                    for each_proposer_block_hash in proposer_block_hash_vec {
                        if self.blockchain.lock().unwrap().blocks.contains_key(&each_proposer_block_hash) == true {
                            peer.write(Message::ProposerBlocks(vec![self.blockchain.lock().unwrap().blocks[&each_proposer_block_hash].clone()]));
                        }
                    }
                }

                Message::ProposerBlocks(proposer_block_vec) => {
                    for each_proposer_block in proposer_block_vec {
                        // if this proposer block is not in the blockchain
                        if self.blockchain.lock().unwrap().blocks.contains_key(&each_proposer_block.hash()) == true{
                            continue;
                        }

                        // PoW check: check if block.hash() <= difficulty. 
                        if each_proposer_block.hash() > each_proposer_block.header.proposer_difficulty {
                            debug!("Proposer wrong sortition");
                            continue;
                        }

                        // If the proposer block is empty, then ignore
                        // commented out for attack
                        /*
                        if each_proposer_block.content.proposer_content.len() == 0 {
                            debug!("Empty Proposer Block");
                            continue;
                        }
                        */

                        // If a block's parent is missing, put this block into a buffer and send Getblocks message.
                        // When the parent is received, that block can be popped out from buffer and check again.
                        if self.blockchain.lock().unwrap().blocks.contains_key(&each_proposer_block.header.proposer_parent) == false {
                            debug!("orphan");
                            if self.orphan_handler.lock().unwrap().contains_key(&each_proposer_block.header.proposer_parent) {
                                if let Some(x) = self.orphan_handler.lock().unwrap().get_mut(&each_proposer_block.header.proposer_parent) {
                                    x.push(each_proposer_block.clone());
                                }
                            }
                            else {
                                self.orphan_handler.lock().unwrap().insert(each_proposer_block.header.proposer_parent, vec![each_proposer_block.clone()]);
                            }
                            peer.write(Message::GetProposerBlocks(vec![each_proposer_block.header.proposer_parent.clone()]));
                            continue;
                        }

                        // When receiving and processing a block,
                        // check if the transaction is signed correctly by the public key(s).
                        let mut valid = true;
                        for each_ref in each_proposer_block.content.proposer_content.clone() {
                            let curr_tx_block = self.tx_block_pool.lock().unwrap()[&each_ref].clone();
                            for each_tx in curr_tx_block.content.transaction_content.iter() {
                                if verify(&each_tx.transaction, &each_tx.sig.key, &each_tx.sig.signature) == false {
                                    debug!("Transaction signed wrong");
                                    valid = false;
                                    break;
                                }        
                            }
                            if valid == false {
                                break;
                            }
                        }
                        if valid == false {
                            continue;
                        }
                        // check the public key(s) matches the owner(s)'s address of these inputs.
                        // check if the inputs to the transactions are not spent
                        // check the values of inputs are not less than those of outputs.

                        // update tx_block_ref
                        let mut new_ref : Vec<H256> = self.tx_block_ref.lock().unwrap()[&each_proposer_block.header.proposer_parent].clone();
                        let my_ref : Vec<H256> = each_proposer_block.content.proposer_content.clone();
                        for each_ref in my_ref.iter() {
                            if new_ref.contains(&each_ref) == false {
                                new_ref.push(*each_ref);
                            }
                        }
                        self.tx_block_ref.lock().unwrap().insert(each_proposer_block.hash(), new_ref.clone());

                        // update state ledger
                        let mut curr_state = self.glob_state.lock().unwrap()[&each_proposer_block.header.proposer_parent].clone();
                        for each_ref in my_ref.iter() {
                            let curr_tx_block = self.tx_block_pool.lock().unwrap()[&each_ref].clone();
                            for each_tx in curr_tx_block.content.transaction_content.iter() {
                                let mut lookup_key = (H256([0;32]),0); // initialized to be a fake look up key
                                let mut exist = false;
                                for i in 0..each_tx.transaction.input_previous.len() {
                                    lookup_key = (each_tx.transaction.input_previous[i], each_tx.transaction.input_index[i]);
                                    // if this transaction is valid, then update the ledger // else not update the ledger
                                    if curr_state.contains_key(&lookup_key) {
                                        exist = true;
                                        curr_state.insert((each_tx.hash(), i as u32), (each_tx.transaction.output_value[i], each_tx.transaction.output_address[i]));
                                    }
                                }
                                if exist == true {
                                    curr_state.remove(&lookup_key);
                                }
                            }
                        }

                        // update transaction pool, remove the transactions that are conflict with the new state.
                        let mut curr_tx_pool = self.tx_pool.lock().unwrap();
                        let mut to_remove_tx : Vec<H256> = Vec::new();
                        for each_tx in curr_tx_pool.keys() {
                            for idx in 0..curr_tx_pool[&each_tx].transaction.input_previous.len() {
                                let lookup_key = (curr_tx_pool[&each_tx].transaction.input_previous[idx],
                                                curr_tx_pool[&each_tx].transaction.input_index[idx]);
                                if curr_state.contains_key(&lookup_key) == false {
                                    to_remove_tx.push(*each_tx);
                                }
                            }
                        }
                        for key in to_remove_tx.iter() {
                            // debug!("Removing old transactions");
                            curr_tx_pool.remove(key);
                        }
                        std::mem::drop(curr_tx_pool);

                        // insert new state
                        self.glob_state.lock().unwrap().insert(each_proposer_block.hash(), curr_state.clone());

                        // insert new block
                        self.blockchain.lock().unwrap().insert(&each_proposer_block.clone());

                        // Relay blocks
                        self.server.broadcast(Message::NewProposerBlockHashes(vec![each_proposer_block.hash()]));

                        // check new parent
                        self.new_parent(&each_proposer_block, &peer);
                    }
                }
            }
        }
    }
}
