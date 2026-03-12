use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Opaque error from the credential vault.
#[derive(Debug, Clone)]
pub struct VaultError(pub String);

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vault error: {}", self.0)
    }
}
impl std::error::Error for VaultError {}

/// Encrypted storage payload for a single secret.
#[derive(Debug, Serialize, Deserialize)]
struct EncryptedSecret {
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    allowed_domains: Vec<String>,
}

/// The encrypted vault file format.
#[derive(Debug, Default, Serialize, Deserialize)]
struct VaultFile {
    /// Mapping of `agent_id -> key_name -> EncryptedSecret`
    secrets: HashMap<String, HashMap<String, EncryptedSecret>>,
}

/// Cryptographic vault for storing and injecting credentials.
#[derive(Clone)]
pub struct CredentialVault {
    storage_path: PathBuf,
    master_key: Key<Aes256Gcm>,
}

impl std::fmt::Debug for CredentialVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialVault")
            .field("storage_path", &self.storage_path)
            .field("master_key", &"****")
            .finish()
    }
}

impl CredentialVault {
    /// Initialize a new vault at the given path.
    /// In this MVP, we randomly generate a master key on initialization if not provided elsewhere.
    pub fn new<P: AsRef<Path>>(path: P, key_bytes: [u8; 32]) -> Self {
        let storage_path = path.as_ref().to_path_buf();
        if !storage_path.exists() {
            if let Some(p) = storage_path.parent() {
                let _ = fs::create_dir_all(p);
            }
            let empty = VaultFile::default();
            let _ = fs::write(&storage_path, serde_json::to_string(&empty).unwrap());
        }

        let master_key = *Key::<Aes256Gcm>::from_slice(&key_bytes);
        Self {
            storage_path,
            master_key,
        }
    }

    /// Read and parse the vault file.
    fn load_vault(&self) -> Result<VaultFile, VaultError> {
        let bytes =
            fs::read(&self.storage_path).map_err(|e| VaultError(format!("Read fail: {}", e)))?;
        let vault: VaultFile =
            serde_json::from_slice(&bytes).map_err(|e| VaultError(format!("Parse fail: {}", e)))?;
        Ok(vault)
    }

    /// Save the vault file securely.
    fn save_vault(&self, vault: &VaultFile) -> Result<(), VaultError> {
        let json = serde_json::to_string_pretty(vault)
            .map_err(|e| VaultError(format!("Serialize fail: {}", e)))?;
        fs::write(&self.storage_path, json)
            .map_err(|e| VaultError(format!("Write fail: {}", e)))?;
        Ok(())
    }

    /// Store a new secret for a specific agent.
    pub fn store_secret(
        &self,
        agent_id: &str,
        key_name: &str,
        plaintext: &str,
        allowed_domains: Vec<String>,
    ) -> Result<(), VaultError> {
        let cipher = Aes256Gcm::new(&self.master_key);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bits

        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| VaultError(format!("Encryption failed: {}", e)))?;

        let mut vault = self.load_vault()?;
        let agent_secrets = vault.secrets.entry(agent_id.to_string()).or_default();

        agent_secrets.insert(
            key_name.to_string(),
            EncryptedSecret {
                nonce: nonce.to_vec(),
                ciphertext,
                allowed_domains,
            },
        );

        self.save_vault(&vault)
    }

    /// Decrypt the secret Just-In-Time to inject it at a network boundary.
    pub fn retrieve_for_egress(
        &self,
        agent_id: &str,
        key_name: &str,
        target_domain: &str,
    ) -> Result<String, VaultError> {
        let vault = self.load_vault()?;
        let agent_secrets = vault
            .secrets
            .get(agent_id)
            .ok_or_else(|| VaultError("Agent not found in vault".to_string()))?;

        let enc_secret = agent_secrets
            .get(key_name)
            .ok_or_else(|| VaultError("Key not found in vault".to_string()))?;

        // Validate domain access control
        let domain_allowed = enc_secret
            .allowed_domains
            .iter()
            .any(|d| target_domain.ends_with(d));
        if !domain_allowed {
            return Err(VaultError(format!(
                "Domain '{}' not authorized for key '{}'",
                target_domain, key_name
            )));
        }

        let cipher = Aes256Gcm::new(&self.master_key);
        let nonce = Nonce::from_slice(&enc_secret.nonce);

        let plaintext_bytes = cipher
            .decrypt(nonce, enc_secret.ciphertext.as_ref())
            .map_err(|e| VaultError(format!("Decryption failed: {}", e)))?;

        String::from_utf8(plaintext_bytes)
            .map_err(|e| VaultError(format!("UTF-8 decode fail: {}", e)))
    }

    /// Convenience wrapper for global system-level secrets.
    pub fn retrieve_global_secret(
        &self,
        key_name: &str,
        target_domain: &str,
    ) -> Result<String, VaultError> {
        self.retrieve_for_egress("system", key_name, target_domain)
    }

    /// Decrypt all secrets in the vault. Useful for populating leak scanners.
    pub fn decrypt_all(&self) -> Result<Vec<String>, VaultError> {
        let vault = self.load_vault()?;
        let cipher = Aes256Gcm::new(&self.master_key);
        let mut all_plaintexts = Vec::new();

        for agent_map in vault.secrets.values() {
            for enc_secret in agent_map.values() {
                let nonce = Nonce::from_slice(&enc_secret.nonce);
                if let Ok(plaintext_bytes) = cipher.decrypt(nonce, enc_secret.ciphertext.as_ref()) {
                    if let Ok(plaintext) = String::from_utf8(plaintext_bytes) {
                        all_plaintexts.push(plaintext);
                    }
                }
            }
        }
        Ok(all_plaintexts)
    }
}
