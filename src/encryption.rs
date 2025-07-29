use anyhow::Result;
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::Aead;
use base64::{Engine as _, engine::general_purpose};
use rand::Rng;
use sha2::{Sha256, Digest};

/// Enhanced encryption wrapper for sensitive data with proper key derivation
pub struct DatabaseEncryption {
    key: Key<Aes256Gcm>,
}

impl DatabaseEncryption {
    /// Create encryption instance from a password with proper key derivation
    pub fn new(password: &str, salt: &str) -> Result<Self> {
        // Use PBKDF2-like key derivation with SHA256
        let mut key_bytes = [0u8; 32];
        let mut hasher = Sha256::new();
        
        // Combine password and salt
        hasher.update(password.as_bytes());
        hasher.update(salt.as_bytes());
        
        // Multiple rounds for better security
        for _ in 0..10000 {
            let result = hasher.finalize_reset();
            hasher.update(&result);
        }
        
        let final_hash = hasher.finalize();
        key_bytes.copy_from_slice(&final_hash[..32]);
        
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        Ok(DatabaseEncryption { key: *key })
    }

    /// Generate a random salt for password hashing
    pub fn generate_salt() -> String {
        let mut salt_bytes = [0u8; 32];
        rand::thread_rng().fill(&mut salt_bytes);
        general_purpose::STANDARD.encode(salt_bytes)
    }

    /// Hash a password with a salt (for storing password hashes)
    pub fn hash_password(password: &str, salt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        hasher.update(salt.as_bytes());
        
        // Multiple rounds for better security
        for _ in 0..10000 {
            let result = hasher.finalize_reset();
            hasher.update(&result);
        }
        
        let final_hash = hasher.finalize();
        general_purpose::STANDARD.encode(final_hash)
    }

    /// Verify a password against a stored hash
    pub fn verify_password(password: &str, salt: &str, stored_hash: &str) -> bool {
        let computed_hash = Self::hash_password(password, salt);
        computed_hash == stored_hash
    }

    pub fn encrypt(&self, data: &str) -> Result<String> {
        let cipher = Aes256Gcm::new(&self.key);
        
        // Generate a random nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        // Encrypt the data
        let ciphertext = cipher.encrypt(nonce, data.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        
        // Combine nonce and ciphertext and encode as base64
        let mut combined = Vec::new();
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        
        Ok(general_purpose::STANDARD.encode(combined))
    }

    pub fn decrypt(&self, encrypted_data: &str) -> Result<String> {
        let cipher = Aes256Gcm::new(&self.key);
        
        // Decode from base64
        let combined = general_purpose::STANDARD.decode(encrypted_data)
            .map_err(|e| anyhow::anyhow!("Base64 decode failed: {}", e))?;
        
        if combined.len() < 12 {
            return Err(anyhow::anyhow!("Invalid encrypted data"));
        }
        
        // Extract nonce and ciphertext
        let nonce_bytes = &combined[..12];
        let ciphertext = &combined[12..];
        
        let nonce = Nonce::from_slice(nonce_bytes);
        
        // Decrypt the data
        let plaintext = cipher.decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;
        
        String::from_utf8(plaintext)
            .map_err(|e| anyhow::anyhow!("Invalid UTF-8: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_decryption() {
        let salt = DatabaseEncryption::generate_salt();
        let encryption = DatabaseEncryption::new("test_password", &salt).unwrap();
        let original_data = "sensitive financial data";
        
        let encrypted = encryption.encrypt(original_data).unwrap();
        let decrypted = encryption.decrypt(&encrypted).unwrap();
        
        assert_eq!(original_data, decrypted);
    }

    #[test]
    fn test_password_hashing() {
        let password = "my_secure_password";
        let salt = DatabaseEncryption::generate_salt();
        
        let hash1 = DatabaseEncryption::hash_password(password, &salt);
        let hash2 = DatabaseEncryption::hash_password(password, &salt);
        
        // Same password and salt should produce same hash
        assert_eq!(hash1, hash2);
        
        // Different salt should produce different hash
        let different_salt = DatabaseEncryption::generate_salt();
        let hash3 = DatabaseEncryption::hash_password(password, &different_salt);
        assert_ne!(hash1, hash3);
        
        // Verify password should work
        assert!(DatabaseEncryption::verify_password(password, &salt, &hash1));
        assert!(!DatabaseEncryption::verify_password("wrong_password", &salt, &hash1));
    }
}