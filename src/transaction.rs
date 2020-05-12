use crate::crypto::hash::{H256, H160, Hashable};
use serde::{Serialize, Deserialize};
use ring::signature::{self, Ed25519KeyPair, Signature, KeyPair};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct SignedTransaction {
    pub transaction: Transaction,
    pub sig: Sign,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Sign {
    pub signature: Vec<u8>,
    pub key: Vec<u8>,
}

impl Hashable for SignedTransaction {
    fn hash(&self) -> H256 {
        let serialized = serde_json::to_string(self).unwrap();
        ring::digest::digest(&ring::digest::SHA256, serialized.as_bytes()).into()
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Transaction {
    pub input_previous: Vec<H256>,
    pub input_index: Vec<u32>,
    pub output_value: Vec<u32>,
    pub output_address: Vec<H160>,
}

impl Hashable for Transaction {
    fn hash(&self) -> H256 {
        let serialized = serde_json::to_string(self).unwrap();
        ring::digest::digest(&ring::digest::SHA256, serialized.as_bytes()).into()
    }
}

/// Create digital signature of a transaction
pub fn sign(t: &Transaction, key: &Ed25519KeyPair) -> Signature {
    let serialized = serde_json::to_string(&t).unwrap();
    let sig = key.sign(serialized.as_bytes());
    return sig;
}

/// Verify digital signature of a transaction, using public key instead of secret key
pub fn verify(t: &Transaction, public_key: &Vec<u8>, signature: &Vec<u8>) -> bool {
    let serialized = serde_json::to_string(&t).unwrap();
    let peer_public_key = signature::UnparsedPublicKey::new(&signature::ED25519, &public_key[..]);
    return peer_public_key.verify(serialized.as_bytes(), &signature[..]).is_ok();
}

pub fn generate_random_transaction(key: &Ed25519KeyPair) -> SignedTransaction {
    let trans: Transaction = Transaction { 
        input_previous: Vec::new(), 
        input_index: Vec::new(),
        output_address: Vec::new(),
        output_value: Vec::new(), 
    };

    let signature = sign(&trans, key);

    let s = Sign {
        signature: signature.as_ref().to_vec(),
        key: key.public_key().as_ref().to_vec(),
    };

    let output = SignedTransaction {
        transaction: trans,
        sig: s,
    };

    return output;
}

#[cfg(any(test, test_utilities))]
mod tests {
    use super::*;
    use crate::crypto::key_pair;

    pub fn generate_random_transaction() -> SignedTransaction {
		let trans: Transaction = Transaction { 
            input_previous: Vec::new(), 
            input_index: Vec::new(),
            output_address: Vec::new(),
            output_value: Vec::new(),
        };

        let key = key_pair::random();

        let signature = sign(&trans, &key);

        let s = Sign {
            signature: signature.as_ref().to_vec(),
            key: key.public_key().as_ref().to_vec(),
        };

        let h : H256 = ring::digest::digest(&ring::digest::SHA256, key.public_key().as_ref()).into();

        let _k = H160::from(h);
        
        let output = SignedTransaction {
            transaction: trans,
            sig: s,
        };
        
		return output;
    }

    #[test]
    fn sign_verify() {
        let output = generate_random_transaction();
        assert!(verify(&output.transaction, &output.sig.key, &output.sig.signature));
    }
}
