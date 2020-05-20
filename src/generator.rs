use crate::blockchain::Blockchain;
use crate::crypto::hash::{H256,H160,Hashable};
use crate::crypto::key_pair;
use crate::network::server::Handle as ServerHandle;
use crate::network::message::Message;
use crate::transaction::{SignedTransaction, Transaction, sign, Sign};

use crossbeam::channel::{unbounded, Receiver, Sender, TryRecvError};
use log::{info};
use ring::signature::KeyPair;
use ring::signature::Ed25519KeyPair;
use rand::random;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;

enum ControlSignal {
    Start(u64), // the number controls the lambda of interval between block generation
    Paused,
    Exit,
    ICO,
    Balance,
    PrintAddr,
    PrintICO,
    PrintTransaction,
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
    glob_state: Arc<Mutex<HashMap<H256, HashMap<(H256,u32), (u32, H160)>>>>,
    all_addr: Arc<Mutex<Vec<H160>>>,
    my_addr_key: Arc<Mutex<HashMap<H160, Ed25519KeyPair>>>,
}

#[derive(Clone)]
pub struct Handle {
    /// Channel for sending signal to the generator thread
    control_chan: Sender<ControlSignal>,
}

pub fn new(
    server: &ServerHandle,
    blockchain: &Arc<Mutex<Blockchain>>,
    tx_pool: &Arc<Mutex<HashMap<H256, SignedTransaction>>>,
    glob_state: &Arc<Mutex<HashMap<H256, HashMap<(H256,u32), (u32, H160)>>>>,
    all_addr: &Arc<Mutex<Vec<H160>>>,
    my_addr_key: &Arc<Mutex<HashMap<H160, Ed25519KeyPair>>>,
) -> (Context, Handle) {
    let (signal_chan_sender, signal_chan_receiver) = unbounded();

    let ctx = Context {
        control_chan: signal_chan_receiver,
        operating_state: OperatingState::Paused,
        server: server.clone(),
        blockchain: Arc::clone(blockchain),
        tx_pool: Arc::clone(tx_pool),
        glob_state: Arc::clone(glob_state),
        all_addr: Arc::clone(all_addr),
        my_addr_key: Arc::clone(my_addr_key),
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

    pub fn pause(&self) {
        self.control_chan.send(ControlSignal::Paused).unwrap();
    }

    pub fn start(&self, lambda: u64) {
        self.control_chan.send(ControlSignal::Start(lambda)).unwrap();
    }

    pub fn balance(&self) {
        self.control_chan.send(ControlSignal::Balance).unwrap();
    }

    pub fn print_addr(&self) {
        self.control_chan.send(ControlSignal::PrintAddr).unwrap();
    }

    pub fn ico(&self) {
        self.control_chan.send(ControlSignal::ICO).unwrap();
    }

    pub fn print_ico(&self) {
        self.control_chan.send(ControlSignal::PrintICO).unwrap();
    }

    pub fn print_transaction(&self) {
        self.control_chan.send(ControlSignal::PrintTransaction).unwrap();
    }
}

impl Context {
    pub fn start(mut self) {
        thread::Builder::new()
            .name("generator".to_string())
            .spawn(move || {
                self.generator_loop();
            })
            .unwrap();
        info!("generator initialized into paused mode");
    }

    fn handle_control_signal(&mut self, signal: ControlSignal) {
        match signal {
            ControlSignal::Exit => {
                info!("Generator shutting down");
                self.operating_state = OperatingState::ShutDown;
            }

            ControlSignal::Start(i) => {
                info!("Generator starting in continuous mode with lambda {}", i);
                self.operating_state = OperatingState::Run(i);
            }

            ControlSignal::Paused => {
                info!("Generator Paused");
                self.operating_state = OperatingState::Paused;
            }

            // used to check money in each wallet
            ControlSignal::Balance => {
                let tips = self.blockchain.lock().unwrap().tip();
                let curr_state = self.glob_state.lock().unwrap()[&tips].clone();
                let mut my_addr_money : HashMap<H160, u32> = HashMap::new();

                for ((_prev_trans, _idx), (value, recipient)) in curr_state {
                    // if this money is for me
                    if self.my_addr_key.lock().unwrap().contains_key(&recipient) {
                        if my_addr_money.contains_key(&recipient) == true {
                            if let Some(x) = my_addr_money.get_mut(&recipient) {
                                *x += value;
                            }
                        }
                        else {
                            my_addr_money.insert(recipient, value);
                        }
                    }
                }
                let mut total = 0;
                for (_each_addr, money) in my_addr_money {
                    total += money
                }
                println!("For me, all address total money is: {:?}", total);
            }

            // used to check initial addresses
            ControlSignal::PrintAddr => {
                println!("all_addr:");
                for each_addr in self.all_addr.lock().unwrap().clone(){
                    println!("{:?}", each_addr);
                }
            
                println!("my_addr_key:");
                for (each_addr, each_key) in self.my_addr_key.lock().unwrap().iter(){
                    println!("{:?}", each_addr);
                    println!("{:?}", each_key);
                }
            }

            // Initial Coin Offering
            ControlSignal::ICO => {
                // before generating the transactions, the glob_state for all nodes should have the same state in genesis block
                // glob state HashMap<genesis_hash, HashMap<(transaction_hash, output index), (value, recipient)>>
                // create a state:
                // input previous does not matter, index range from 0 to len(addrs), values are 1000, recipients are all addresses
                
                let genesis_hash = self.blockchain.lock().unwrap().tip();
                let mut ico_state : HashMap<(H256, u32), (u32, H160)> = HashMap::new();
                let length = self.all_addr.lock().unwrap().len();
                for i in 0..length {
                    ico_state.insert((H256::from([0;32]), i as u32), (1000, self.all_addr.lock().unwrap()[i]));
                }
                self.glob_state.lock().unwrap().insert(genesis_hash, ico_state.clone());
                self.server.broadcast(Message::ICO(genesis_hash, ico_state.clone()));
            }

            // Check if ICO is initialized correctly
            ControlSignal::PrintICO => {
                let tip = self.blockchain.lock().unwrap().tip();
                let curr_state = self.glob_state.lock().unwrap()[&tip].clone();
                for ((prev_trans, idx), (value, recipient)) in curr_state.iter() {
                    println!("prev_trans:{:?}, idx: {:?}, value: {:?}, recipient: {:?}", prev_trans, idx, value, recipient);
                }
            }

            // Check the transaction pool
            ControlSignal::PrintTransaction => {
                let m = self.tx_pool.lock().unwrap();
                println!("Transaction Mempool size: {:?}", m.len());
                for (_, item) in m.clone() {
                    println!("Transactions: {:?}", item.transaction);
                }
            }
        }
    }
    fn generator_loop(&mut self) {
        // this is initializing addresses and broadcasting to peers
        // each node controls some addresses, put into my_addr_key
        // and all the addresses are stored into the all_addr
        let addr_num = 30;
        for _i in 0..addr_num {
            let key = key_pair::random();
            let h : H256 = ring::digest::digest(&ring::digest::SHA256, key.public_key().as_ref()).into();
            let addr = H160::from(h);
            self.my_addr_key.lock().unwrap().insert(addr, key);
            self.all_addr.lock().unwrap().push(addr);
            self.server.broadcast(Message::NewAddr(self.all_addr.lock().unwrap().to_vec()));
        }
        
        // this is the fake last state to make sure that the generator enter first loop.
        let mut last_state : H256 = H256::from([0;32]);

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
                    Err(TryRecvError::Disconnected) => panic!("Generator control channel detached"),
                },
            }

            if let OperatingState::ShutDown = self.operating_state {
                return;
            }

            // when the generator start:
            // check the latest state, in blockchain.tip(), if the state is not changed, do not generate new transaction
            let curr_state : H256 = self.blockchain.lock().unwrap().tip();
            
            if last_state == curr_state {
                continue;
            }
            last_state = curr_state;

            // new transaction:
            // for each UTXO, give it a new recipient with new value and return rest of the money back to myself
            // E.g. UTXO:             000...000 0 1000 addr1
            //      New Transaction:  000...000 0 2    addr2
            //                        000...000 1 998  addr1
            // When this new transaction is included in the state:
            //      (000...000 0) is removed, (hash(new transaction), 0) and (hash(new transaction), 1) are added into state
            let current_state = self.glob_state.lock().unwrap()[&curr_state].clone();
            for ((prev_trans, idx), (value, recipient)) in current_state.iter() {
                // if this recipient is not controlled by me, then I cannot spend it
                if self.my_addr_key.lock().unwrap().contains_key(&*recipient) == false {
                    continue;
                }

                // if this coin is 0 value, then discard it
                if *value == 0 {
                    continue;
                }

                // for each new transaction, contain 2 entries:
                // first one is some money to other person
                // second one is rest of the money to myself
                let mut input_vec : Vec<H256> = Vec::new();
                input_vec.push(*prev_trans);
                input_vec.push(*prev_trans);

                let mut idx_vec : Vec<u32> = Vec::new();
                idx_vec.push(*idx);
                idx_vec.push(*idx);

                let mut addr_vec : Vec<H160> = Vec::new();
                // one is to other person
                let dest_idx : usize = (random::<u32>() as usize) % (self.all_addr.lock().unwrap().len()) ;
                addr_vec.push(self.all_addr.lock().unwrap()[dest_idx]);
                // one is to myself
                addr_vec.push(*recipient);

                let mut val_vec : Vec<u32> = Vec::new();
                // some money to other person
                let val = 1 + random::<u32>() % *value;
                val_vec.push(val);
                // rest money to myself
                val_vec.push(*value - val);

                let trans : Transaction = Transaction {
                    input_previous: input_vec.clone(),
                    input_index: idx_vec.clone(),
                    output_address: addr_vec.clone(),
                    output_value: val_vec.clone(),
                };

                let my_key = &self.my_addr_key.lock().unwrap()[recipient];
                let signature = sign(&trans, &my_key);

                let s = Sign {
                    signature: signature.as_ref().to_vec().clone(),
                    key: my_key.public_key().as_ref().to_vec(),
                };

                let new_trans = SignedTransaction {
                    transaction: trans.clone(),
                    sig: s.clone(),
                };

                self.tx_pool.lock().unwrap().insert(new_trans.hash(), new_trans.clone());
                self.server.broadcast(Message::NewTransactionHashes(vec![new_trans.hash()]));
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