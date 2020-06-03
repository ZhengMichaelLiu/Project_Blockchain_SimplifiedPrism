# Simplified Prism 1.0 Protocol

This is the final project of ECE598: Principles of Principles of Blockchains, UIUC Spring 2020.

We implemented a simplified Prism 1.0 Protocol to improve throughput of the Bitcoin Client. 

[Click here](https://arxiv.org/pdf/1909.11261.pdf) to read the paper: **_Prism: Scaling Bitcoin by 10,000×, Yang et al. (2020)_**

Compared with the original Prism 1.0 Protocol described in above paper, our simplified version only has the transaction block part and proposer block part, without confirmation block part. 

## Installation

First, install [Rust](https://www.rust-lang.org/tools/install) and Cargo to set up the environment

On Linux and macOS systems, this is done as follows:

```bash
$ curl https://sh.rustup.rs -sSf | sh
```

It will download a script, and start the installation. If everything goes well, you’ll see this appear:

```bash
Rust is installed now. Great!
```

After this, you can use the `rustup` command to also install `beta` or `nightly` channels for Rust and Cargo.

## Usage

After downloading the source code, follow the steps listed below to run the project:

1. Open up 3 termimals and start three processes of the program by running these three commands, respectively:

```bash
cargo run -- -vvv --p2p 127.0.0.1:6000 --api 127.0.0.1:7000
cargo run -- -vvv --p2p 127.0.0.1:6001 --api 127.0.0.1:7001 -c 127.0.0.1:6000
cargo run -- -vvv --p2p 127.0.0.1:6002 --api 127.0.0.1:7002 -c 127.0.0.1:6001
```

`--p2p` parameter means that the first process will listen on `127.0.0.1:6000` and the second process will listen on `127.0.0.1:6001`.

`-c` parameter means that the second process will try to connect to `127.0.0.1:6000`, which is the address of the first process.

On the first process, you can see this log, indicating that the first process accepts connection from the second process.

```bash
New incoming connection from ...
```

2. Open up another terminal and within it, use the following API to do "initial coin offering" to give some money to each address controlled by each process.

```bash
curl http://127.0.0.1:7000/generator/ico
```

3. Then you can use the following API to check how much money each process has:

```bash
curl http://127.0.0.1:7000/generator/balance http://127.0.0.1:7001/generator/balance http://127.0.0.1:7002/generator/balance
```

4. Then use the following API to start the transaction generator:
```bash
curl "http://127.0.0.1:7000/generator/start?lambda=1000000" "http://127.0.0.1:7001/generator/start?lambda=1000000" "http://127.0.0.1:7002/generator/start?lambda=1000000"
```

5. Then use the following API to start the miner:
```bash
curl "http://127.0.0.1:7000/miner/start?lambda=1000000" "http://127.0.0.1:7001/miner/start?lambda=1000000" "http://127.0.0.1:7002/miner/start?lambda=1000000"   
```

6. Again, you can use the following API to check how much money each process has:

```bash
curl http://127.0.0.1:7000/generator/balance http://127.0.0.1:7001/generator/balance http://127.0.0.1:7002/generator/balance
```

The total amount of the money should be the same as the beginning, but due to the transactions are making between processes, the amount of money in each account is changing as time goes.

## Coworker

Tianyi Tang.
