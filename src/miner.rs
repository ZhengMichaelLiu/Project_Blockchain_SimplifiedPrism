use crate::block::{Header, Content, Block};
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
) -> (Context, Handle) {
    let (signal_chan_sender, signal_chan_receiver) = unbounded();

    let ctx = Context {
        control_chan: signal_chan_receiver,
        operating_state: OperatingState::Paused,
        server: server.clone(),
        blockchain: Arc::clone(blockchain),
        tx_pool: Arc::clone(tx_pool),
        glob_state: Arc::clone(glob_state),
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
        let mut mined_block : u32 = 0;
        let mut confirmed_tx : u32 = 0;
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
            // Mine the content of the block
            // then get the merkle root of the block
            // then make the new block and check the hash sortition
            // if valid, then remove transaction from tx_pool, update state, update tx_pool, insert into blockchain, broadcast

            let num_trans_in_block : u32 = 10;
            let new_parent = self.blockchain.lock().unwrap().tip();

            // Mine the content of block
            let mut new_block_content : Vec<SignedTransaction> = Vec::new();
            let mut curr_state = self.glob_state.lock().unwrap()[&new_parent].clone();

            let mut count_tx = 0;
            let curr_tx_pool = self.tx_pool.lock().unwrap();
            for each_tx in curr_tx_pool.keys() {
                if count_tx >= num_trans_in_block {
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

                // if this transaction is valid, then add to new block content
                if valid == true {
                    new_block_content.push(curr_tx_pool[&each_tx].clone());
                    count_tx += 1;
                }
            }
            std::mem::drop(curr_tx_pool);
            
            let new_difficulty : H256 = self.blockchain.lock().unwrap().blocks[&new_parent].header.difficulty;
            let new_merkle : H256 = MerkleTree::new(&new_block_content).root();

            let new_header : Header = Header {
                parent: new_parent,
                nonce: random::<u32>(),
                timestamp: time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH).unwrap().as_millis(),
                difficulty: new_difficulty,
                merkle: new_merkle,
            };

            let new_content = Content {
                transaction_content: new_block_content.clone(),
            };

            let new_block = Block{
                header: new_header.clone(),
                content: new_content.clone(),
            };

            // if this block is empty, then ignore this block
            if new_block_content.len() == 0 {
                continue;
            }

            // new block
            if new_block.hash() < new_difficulty {
                // remove transactions from mempool
                for each_transaction in new_block_content {
                    if self.tx_pool.lock().unwrap().contains_key(&each_transaction.hash()) == false {
                        continue;
                    }
                    self.tx_pool.lock().unwrap().remove(&each_transaction.hash());
                }

                // and update the ledger state
                for each_tx in new_block.content.transaction_content.iter() {
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

                // if this block is valid, then add this block to block chain
                self.blockchain.lock().unwrap().insert(&new_block.clone());

                self.server.broadcast(Message::NewBlockHashes(vec![new_block.hash()]));

                mined_block += 1;
                debug!("Current Mined Block: {:?}", mined_block);
                debug!("Current Confirmed Transactions in Ledger: {:?}", confirmed_tx);
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
