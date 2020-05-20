#[cfg(test)]
#[macro_use]
extern crate hex_literal;

pub mod api;
pub mod block;
pub mod blockchain;
pub mod crypto;
pub mod generator;
pub mod miner;
pub mod network;
pub mod transaction;

use api::Server as ApiServer;
use clap::clap_app;
use crossbeam::channel;
use log::{error, info};
use network::{server, worker};
use std::net;
use std::process;
use std::thread;
use std::time;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use crate::transaction::SignedTransaction;
use crate::crypto::hash::{H256,H160};
use ring::signature::Ed25519KeyPair;

fn main() {
    // parse command line arguments
    let matches = clap_app!(Bitcoin =>
     (version: "0.1")
     (about: "Bitcoin client")
     (@arg verbose: -v ... "Increases the verbosity of logging")
     (@arg peer_addr: --p2p [ADDR] default_value("127.0.0.1:6000") "Sets the IP address and the port of the P2P server")
     (@arg api_addr: --api [ADDR] default_value("127.0.0.1:7000") "Sets the IP address and the port of the API server")
     (@arg known_peer: -c --connect ... [PEER] "Sets the peers to connect to at start")
     (@arg p2p_workers: --("p2p-workers") [INT] default_value("8") "Sets the number of worker threads for P2P server")
     // if this is an attack, then attacker = 1
     (@arg attacker: --("attacker") [INT] default_value("0") "Set if this is an attacker")
    )
    .get_matches();

    // parse attacker 
    let attacker = matches
        .value_of("attacker")
        .unwrap()
        .parse::<usize>()
        .unwrap_or_else(|e| {
            error!("Error parsing attacker: {}", e);
            process::exit(1);
        });

    // init logger
    let verbosity = matches.occurrences_of("verbose") as usize;
    stderrlog::new().verbosity(verbosity).init().unwrap();

    // parse p2p server address
    let p2p_addr = matches
        .value_of("peer_addr")
        .unwrap()
        .parse::<net::SocketAddr>()
        .unwrap_or_else(|e| {
            error!("Error parsing P2P server address: {}", e);
            process::exit(1);
        });

    // parse api server address
    let api_addr = matches
        .value_of("api_addr")
        .unwrap()
        .parse::<net::SocketAddr>()
        .unwrap_or_else(|e| {
            error!("Error parsing API server address: {}", e);
            process::exit(1);
        });

    // create channels between server and worker
    let (msg_tx, msg_rx) = channel::unbounded();

    // start the p2p server
    let (server_ctx, server) = server::new(p2p_addr, msg_tx).unwrap();
    server_ctx.start().unwrap();

    // start the worker
    let p2p_workers = matches
        .value_of("p2p_workers")
        .unwrap()
        .parse::<usize>()
        .unwrap_or_else(|e| {
            error!("Error parsing P2P workers: {}", e);
            process::exit(1);
        });

    // connect to known peers
    if let Some(known_peers) = matches.values_of("known_peer") {
        let known_peers: Vec<String> = known_peers.map(|x| x.to_owned()).collect();
        let server = server.clone();
        thread::spawn(move || {
            for peer in known_peers {
                loop {
                    let addr = match peer.parse::<net::SocketAddr>() {
                        Ok(x) => x,
                        Err(e) => {
                            error!("Error parsing peer address {}: {}", &peer, e);
                            break;
                        }
                    };
                    match server.connect(addr) {
                        Ok(_) => {
                            info!("Connected to outgoing peer {}", &addr);
                            break;
                        }
                        Err(e) => {
                            error!(
                                "Error connecting to peer {}, retrying in one second: {}",
                                addr, e
                            );
                            thread::sleep(time::Duration::from_millis(1000));
                            continue;
                        }
                    }
                }
            }
        });
    }

    // blockchain
    let chain : blockchain::Blockchain = blockchain::Blockchain::new();
    let blockchain = Arc::new(Mutex::new(chain));

    // orphan block buffer
    let map : HashMap<H256, Vec<block::ProposerBlock>> = HashMap::new();
    let orphan_handler = Arc::new(Mutex::new(map));

    // transaction mempool
    let mempool_map : HashMap<H256, SignedTransaction> = HashMap::new();
    let tx_pool = Arc::new(Mutex::new(mempool_map));

    // global state ledger
    let glob_state_body : HashMap<H256, HashMap<(H256, u32), (u32, H160)>> = HashMap::new();
    let glob_state = Arc::new(Mutex::new(glob_state_body));

    // all address
    let all_addr_body : Vec<H160> = Vec::new();
    let all_addr = Arc::new(Mutex::new(all_addr_body));

    // my address
    let my_addr_key_body : HashMap<H160, Ed25519KeyPair> = HashMap::new();
    let my_addr_key = Arc::new(Mutex::new(my_addr_key_body));

    // transaction block mempool
    let tx_block_pool_map : HashMap<H256, block::TXBlock> = HashMap::new();
    let tx_block_pool = Arc::new(Mutex::new(tx_block_pool_map));

    // record what tx blocks are referenced by this proposer block and its ancestors
    let block_reference_body : HashMap<H256, Vec<H256>> = HashMap::new();
    let tx_block_ref = Arc::new(Mutex::new(block_reference_body));

    let worker_ctx = worker::new(
        p2p_workers,
        msg_rx,
        &server,
        &blockchain,
        &orphan_handler,
        &tx_pool,
        &glob_state,
        &all_addr,
        &my_addr_key,
        &tx_block_pool,
        &tx_block_ref
    );
    worker_ctx.start();

    // start the miner
    let (miner_ctx, miner) = miner::new(
        &server,
        &blockchain,
        &tx_pool,
        &glob_state,
        &tx_block_pool,
        &tx_block_ref
    );
    if attacker == 0 {
        miner_ctx.start();
    }
    else {
        miner_ctx.start_attacker();
    }
    
    // start the generator
    let (generator_ctx, generator) = generator::new(
        &server,
        &blockchain,
        &tx_pool,
        &glob_state,
        &all_addr,
        &my_addr_key,
    );
    generator_ctx.start();

    // start the API server
    ApiServer::start(
        api_addr,
        &miner,
        &generator,
        &server,
    );

    loop {
        std::thread::park();
    }
}
