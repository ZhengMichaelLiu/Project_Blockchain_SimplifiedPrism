use crate::block::{Header, ProposerContent, ProposerBlock, TXContent, TXBlock};
use crate::blockchain::Blockchain;
use crate::crypto::hash::{H256,H160,Hashable};
use crate::crypto::merkle::MerkleTree;
use crate::network::server::Handle as ServerHandle;
use crate::network::message::Message;
use crate::transaction::SignedTransaction;

use crossbeam::channel::{unbounded, Receiver, Sender, TryRecvError};
use log::{debug,info};
use rand::random;
use std::collections::HashMap;
use std::time;
use std::sync::{Arc, Mutex};
use std::thread;

enum ControlSignal {
    Start(u64), // the number controls the lambda of interval between block generation
    Exit,
}

enum OperatingState {
    Paused,
    Run(u64),
    ShutDown,
}

pub struct Context {
    /// Channel for receiving control signal
    control_chan: Receiver<ControlSignal>,
    operating_state: OperatingState,
    server: ServerHandle,
    blockchain: Arc<Mutex<Blockchain>>,
    tx_pool: Arc<Mutex<HashMap<H256, SignedTransaction>>>,
    glob_state: Arc<Mutex<HashMap<H256, HashMap<(H256, u32), (u32, H160)>>>>,
    tx_block_pool: Arc<Mutex<HashMap<H256, TXBlock>>>,
    tx_block_ref: Arc<Mutex<HashMap<H256, Vec<H256>>>>,
}

#[derive(Clone)]
pub struct Handle {
    /// Channel for sending signal to the miner thread
    control_chan: Sender<ControlSignal>,
}

pub fn new(
    server: &ServerHandle,
    blockchain: &Arc<Mutex<Blockchain>>,
    tx_pool: &Arc<Mutex<HashMap<H256, SignedTransaction>>>,
    glob_state: &Arc<Mutex<HashMap<H256, HashMap<(H256, u32), (u32, H160)>>>>,
    tx_block_pool: &Arc<Mutex<HashMap<H256, TXBlock>>>,
    tx_block_ref: &Arc<Mutex<HashMap<H256, Vec<H256>>>>,
) -> (Context, Handle) {
    let (signal_chan_sender, signal_chan_receiver) = unbounded();

    let ctx = Context {
        control_chan: signal_chan_receiver,
        operating_state: OperatingState::Paused,
        server: server.clone(),
        blockchain: Arc::clone(blockchain),
        tx_pool: Arc::clone(tx_pool),
        glob_state: Arc::clone(glob_state),
        tx_block_pool: Arc::clone(tx_block_pool),
        tx_block_ref: Arc::clone(tx_block_ref),
    };
    
    let handle = Handle {
        control_chan: signal_chan_sender,
    };

    (ctx, handle)
}

impl Handle {
    pub fn exit(&self) {
        self.control_chan.send(ControlSignal::Exit).unwrap();
    }

    pub fn start(&self, lambda: u64) {
        self.control_chan.send(ControlSignal::Start(lambda)).unwrap();
    }
}

impl Context {
    pub fn start(mut self) {
        thread::Builder::new()
            .name("miner".to_string())
            .spawn(move || {
                self.miner_loop();
            })
            .unwrap();
        info!("Miner initialized into paused mode");
    }

    pub fn start_attacker(mut self) {
        thread::Builder::new()
            .name("miner attacker".to_string())
            .spawn(move || {
                self.miner_loop_attacker();
            })
            .unwrap();
        info!("Miner Attacker initialized into paused mode");
    }

    fn handle_control_signal(&mut self, signal: ControlSignal) {
        match signal {
            ControlSignal::Exit => {
                info!("Miner shutting down");
                self.operating_state = OperatingState::ShutDown;
            }

            ControlSignal::Start(i) => {
                info!("Miner starting in continuous mode with lambda {}", i);
                self.operating_state = OperatingState::Run(i);
            }
        }
    }

    fn miner_loop(&mut self) {
        // for genesis, tx_block_ref is empty
        let genesis : H256 = self.blockchain.lock().unwrap().tip();
        let empty_ref : Vec<H256> = Vec::new();
        self.tx_block_ref.lock().unwrap().insert(genesis, empty_ref.clone());

        let mut mined_tx_block : u32 = 0;
        let mut mined_pp_block : u32 = 0;
        let mut confirmed_tx   : u32 = 0;
        loop {
            // check and react to control signals
            match self.operating_state {
                OperatingState::Paused => {
                    let signal = self.control_chan.recv().unwrap();
                    self.handle_control_signal(signal);
                    continue;
                }
                OperatingState::ShutDown => {
                    return;
                }
                _ => match self.control_chan.try_recv() {
                    Ok(signal) => {
                        self.handle_control_signal(signal);
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => panic!("Miner control channel detached"),
                },
            }

            if let OperatingState::ShutDown = self.operating_state {
                return;
            }

            // Mining
            // Mine the content of the transaction block
            // Mine the content of the proposer block
            // then get the merkle root of transaction block and proposer block
            // then make the new header
            // then hash the new header, and check the hash sortition
            //      if it is a transaction block, then remove transactions from tx_pool 
            //      and add this new transaction block to tx_block_pool, and broadcast this new transaction block
            //      if it is a proposer block, then update the state based on these transactions and broadcast this new block

            let num_trans_in_trans_block : u32 = 10;
            let num_ref_in_proposer_block : u32 = 10;
            let new_parent = self.blockchain.lock().unwrap().tip();

            // Mine the content of transaction block
            let mut new_trans_block_content : Vec<SignedTransaction> = Vec::new();
            let mut curr_state = self.glob_state.lock().unwrap()[&new_parent].clone();
            let mut count_tx = 0;

            let curr_tx_pool = self.tx_pool.lock().unwrap();
            for each_tx in curr_tx_pool.keys() {
                if count_tx >= num_trans_in_trans_block {
                    break;
                }

                // check if the transaction is valid, maybe double spend
                let mut valid = true;
                for idx in 0..curr_tx_pool[&each_tx].transaction.input_previous.len() {
                    let lookup_key = (curr_tx_pool[&each_tx].transaction.input_previous[idx],
                                      curr_tx_pool[&each_tx].transaction.input_index[idx]);
                    if curr_state.contains_key(&lookup_key) == false {
                        valid = false; 
                        break;
                    }
                }

                // if this transaction is valid, then add to new tx block content
                if valid == true {
                    new_trans_block_content.push(curr_tx_pool[&each_tx].clone());
                    count_tx += 1;
                }
            }
            std::mem::drop(curr_tx_pool);
            
            // Mine the content of proposer block
            let mut new_props_block_content : Vec<H256> = Vec::new();
            let pointed_tx_blocks : Vec<H256> = self.tx_block_ref.lock().unwrap()[&new_parent].clone();
            let mut count_ref : u32 = 0;
            for each_tx_block in self.tx_block_pool.lock().unwrap().keys() {
                if count_ref >= num_ref_in_proposer_block {
                    break;
                }

                // if this tx block is not pointed by ancestors, then it can be pointed by new block
                if pointed_tx_blocks.contains(&each_tx_block) == false {
                    new_props_block_content.push(*each_tx_block);
                    count_ref += 1;
                }
            }
            
            let new_transaction_difficulty : H256 = self.blockchain.lock().unwrap().blocks[&new_parent].header.transaction_difficulty;
            let new_proposer_difficulty : H256 = self.blockchain.lock().unwrap().blocks[&new_parent].header.proposer_difficulty;
            let new_merkle_transaction : H256 = MerkleTree::new(&new_trans_block_content).root();
            let new_merkle_proposer : H256 = MerkleTree::new(&new_props_block_content).root();

            let new_header : Header = Header {
                proposer_parent: new_parent,
                nonce: random::<u32>(),
                timestamp: time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH).unwrap().as_millis(),
                transaction_difficulty: new_transaction_difficulty,
                proposer_difficulty: new_proposer_difficulty,
                merkle_transaction: new_merkle_transaction,
                merkle_proposer: new_merkle_proposer,
            };

            // proposer block
            if new_header.hash() < new_proposer_difficulty {
                // if this proposer block is empty, then ignore this block
                if new_props_block_content.len() == 0 {
                    continue;
                }

                let new_content = ProposerContent {
                    proposer_content: new_props_block_content.clone(),
                };

                let new_block = ProposerBlock {
                    header: new_header.clone(),
                    content: new_content.clone(),
                };

                // and insert new reference state of tx blocks
                let concatenated = [&pointed_tx_blocks[..], &new_props_block_content[..]].concat();
                self.tx_block_ref.lock().unwrap().insert(new_block.hash(), concatenated.clone());

                // and update the ledger state
                for each_ref in new_props_block_content.iter() {
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
                            confirmed_tx += 1;
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
                    curr_tx_pool.remove(key);
                }
                std::mem::drop(curr_tx_pool);

                // add this block and it's state 
                self.glob_state.lock().unwrap().insert(new_block.hash(), curr_state.clone());

                // if this proposer block is valid, then add this block to block chain
                self.blockchain.lock().unwrap().insert(&new_block.clone());

                self.server.broadcast(Message::NewProposerBlockHashes(vec![new_block.hash()]));

                mined_pp_block += 1;
                debug!("Current Mined Transaction Block: {:?}", mined_pp_block);
                debug!("Current Confirmed Transactions in Ledger: {:?}", confirmed_tx);
            }
            // transaction block
            else if new_header.hash() < new_transaction_difficulty {
                // if this transaction block is empty, then ignore this transaction block
                if new_trans_block_content.len() == 0 {
                    continue;
                }

                let new_content = TXContent {
                    transaction_content: new_trans_block_content.clone(),
                };

                let new_block = TXBlock {
                    header : new_header.clone(),
                    content : new_content.clone(),
                };
                
                // if this transaction block is valid, then remove the transactions from mempool
                for each_transaction in new_trans_block_content {
                    if self.tx_pool.lock().unwrap().contains_key(&each_transaction.hash()) == false {
                        continue;
                    }
                    self.tx_pool.lock().unwrap().remove(&each_transaction.hash());
                }

                // add this new transaction block into the transaction block pool
                self.tx_block_pool.lock().unwrap().insert(new_block.hash(), new_block.clone());

                // broadcast to other nodes
                self.server.broadcast(Message::NewTransactionBlockHashes(vec![new_block.hash()]));

                mined_tx_block += 1;
                debug!("Current Mined Transaction Block: {:?}", mined_tx_block);
            }

            if let OperatingState::Run(i) = self.operating_state {
                if i != 0 {
                    let interval = time::Duration::from_micros(i as u64);
                    thread::sleep(interval);
                }
            }
        }
    }

    fn miner_loop_attacker(&mut self) {
        // for genesis, tx_block_ref is empty
        let genesis : H256 = self.blockchain.lock().unwrap().tip();
        let empty_ref : Vec<H256> = Vec::new();
        self.tx_block_ref.lock().unwrap().insert(genesis, empty_ref.clone());

        let mut mined_tx_block : u32 = 0;
        let mut mined_pp_block : u32 = 0;
        let confirmed_tx : u32 = 0;

        loop {
            // check and react to control signals
            match self.operating_state {
                OperatingState::Paused => {
                    let signal = self.control_chan.recv().unwrap();
                    self.handle_control_signal(signal);
                    continue;
                }
                OperatingState::ShutDown => {
                    return;
                }
                _ => match self.control_chan.try_recv() {
                    Ok(signal) => {
                        self.handle_control_signal(signal);
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => panic!("Miner control channel detached"),
                },
            }

            if let OperatingState::ShutDown = self.operating_state {
                return;
            }

            // Mining
            // Mine the content of the transaction block
            // Mine the content of the proposer block
            // then get the merkle root of transaction block and proposer block
            // then make the new header
            // then hash the new header, and check the hash sortition
            //      if it is a transaction block, then remove transactions from tx_pool 
            //      and add this new transaction block to tx_block_pool, and broadcast this new transaction block
            //      if it is a proposer block, then update the state based on these transactions and broadcast this new block

            let new_parent = self.blockchain.lock().unwrap().tip();

            // Mine the content of transaction block
            let new_trans_block_content : Vec<SignedTransaction> = Vec::new();
            let curr_state = self.glob_state.lock().unwrap()[&new_parent].clone();
            
            // Mine the content of proposer block
            let new_props_block_content : Vec<H256> = Vec::new();
            let pointed_tx_blocks : Vec<H256> = self.tx_block_ref.lock().unwrap()[&new_parent].clone();
            
            let new_transaction_difficulty : H256 = self.blockchain.lock().unwrap().blocks[&new_parent].header.transaction_difficulty;
            let new_proposer_difficulty : H256 = self.blockchain.lock().unwrap().blocks[&new_parent].header.proposer_difficulty;
            let new_merkle_transaction : H256 = MerkleTree::new(&new_trans_block_content).root();
            let new_merkle_proposer : H256 = MerkleTree::new(&new_props_block_content).root();

            let new_header : Header = Header {
                proposer_parent: new_parent,
                nonce: random::<u32>(),
                timestamp: time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH).unwrap().as_millis(),
                transaction_difficulty: new_transaction_difficulty,
                proposer_difficulty: new_proposer_difficulty,
                merkle_transaction: new_merkle_transaction,
                merkle_proposer: new_merkle_proposer,
            };

            // proposer block
            if new_header.hash() < new_proposer_difficulty {
                let new_content = ProposerContent {
                    proposer_content: new_props_block_content.clone(),
                };

                let new_block = ProposerBlock {
                    header: new_header.clone(),
                    content: new_content.clone(),
                };

                // and insert new reference state of tx blocks
                let concatenated = [&pointed_tx_blocks[..], &new_props_block_content[..]].concat();
                self.tx_block_ref.lock().unwrap().insert(new_block.hash(), concatenated.clone());

                // update nothing

                // add this block and it's state 
                self.glob_state.lock().unwrap().insert(new_block.hash(), curr_state.clone());

                // if this proposer block is valid, then add this block to block chain
                self.blockchain.lock().unwrap().insert(&new_block.clone());

                self.server.broadcast(Message::NewProposerBlockHashes(vec![new_block.hash()]));

                mined_pp_block += 1;
                debug!("Current Mined Transaction Block: {:?}", mined_pp_block);
                debug!("Current Confirmed Transactions in Ledger: {:?}", confirmed_tx);
            }
            // transaction block
            else if new_header.hash() < new_transaction_difficulty {
                let new_content = TXContent {
                    transaction_content: new_trans_block_content.clone(),
                };

                let new_block = TXBlock {
                    header : new_header.clone(),
                    content : new_content.clone(),
                };

                // because this block is empty, no need to remove transaction from mempool

                // add this new transaction block into the transaction block pool
                self.tx_block_pool.lock().unwrap().insert(new_block.hash(), new_block.clone());

                // broadcast to other nodes
                self.server.broadcast(Message::NewTransactionBlockHashes(vec![new_block.hash()]));

                mined_tx_block += 1;
                debug!("Current Mined Transaction Block: {:?}", mined_tx_block);
            }

            if let OperatingState::Run(i) = self.operating_state {
                if i != 0 {
                    let interval = time::Duration::from_micros(i as u64);
                    thread::sleep(interval);
                }
            }
        }
    }
}
