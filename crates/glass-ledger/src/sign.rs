use std::fs;
use std::path::Path;

use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use rand::rngs::OsRng;

pub struct Signer {
    pub key_id: String,
    signing_key: SigningKey,
    pub public_pem: String,
}

pub fn load_or_create_signer(key_path: &Path) -> std::io::Result<Signer> {
    if key_path.exists() {
        let data = fs::read(key_path)?;
        if data.len() >= 32 {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&data[..32]);
            let signing_key = SigningKey::from_bytes(&seed);
            let verifying_key = signing_key.verifying_key();
            return Ok(Signer {
                key_id: hex::encode(verifying_key.as_bytes()),
                signing_key,
                public_pem: pem_encode_public(verifying_key.as_bytes()),
            });
        }
    }
    let signing_key = SigningKey::generate(&mut OsRng);
    let seed = signing_key.to_bytes();
    fs::write(key_path, seed)?;
    let verifying_key = signing_key.verifying_key();
    Ok(Signer {
        key_id: hex::encode(verifying_key.as_bytes()),
        signing_key,
        public_pem: pem_encode_public(verifying_key.as_bytes()),
    })
}

impl Signer {
    pub fn sign(&self, canonical: &str) -> String {
        let sig = self.signing_key.sign(canonical.as_bytes());
        hex::encode(sig.to_bytes())
    }
}

pub fn verify_signature(public_pem: &str, message: &str, sig_hex: &str) -> bool {
    let Ok(bytes) = hex::decode(sig_hex) else {
        return false;
    };
    if bytes.len() != 64 {
        return false;
    }
    let Ok(pub_bytes) = pem_decode_public(public_pem) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&pub_bytes) else {
        return false;
    };
    let Ok(sig) = ed25519_dalek::Signature::from_slice(&bytes) else {
        return false;
    };
    use ed25519_dalek::Verifier;
    verifying_key.verify(message.as_bytes(), &sig).is_ok()
}

fn pem_encode_public(bytes: &[u8; 32]) -> String {
    format!(
        "-----BEGIN OMNIS GLASS PUBLIC KEY-----\n{}\n-----END OMNIS GLASS PUBLIC KEY-----\n",
        hex::encode(bytes)
    )
}

fn pem_decode_public(pem: &str) -> Result<[u8; 32], ()> {
    let line = pem
        .lines()
        .find(|l| !l.starts_with("-----"))
        .ok_or(())?;
    let bytes = hex::decode(line.trim()).map_err(|_| ())?;
    if bytes.len() != 32 {
        return Err(());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}
