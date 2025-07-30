use anyhow::Result;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use crate::encryption::DatabaseEncryption;
use log::{info, warn, error};

const KEYRING_SERVICE: &str = "MyPersonalApplicationsService";
const KEYRING_USER: &str = "preft";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    pub enabled: bool,
    pub password_hash: Option<String>,
    pub salt: Option<String>,
    pub database_encrypted: bool,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Default to encryption enabled
            password_hash: None,
            salt: None,
            database_encrypted: false,
        }
    }
}

impl EncryptionConfig {
    /// Load encryption configuration from OS keystore
    pub fn load() -> Result<Self> {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
        
        match entry.get_password() {
            Ok(encrypted_config) => {
                // The config itself is stored in plain text in the keystore
                // since the keystore is already secure
                let config: EncryptionConfig = serde_json::from_str(&encrypted_config)?;
                Ok(config)
            }
            Err(keyring::Error::NoEntry) => {
                // No configuration exists yet, return default
                Ok(EncryptionConfig::default())
            }
            Err(e) => {
                // Other error, return default but log the issue
                log::error!("Warning: Could not load encryption config from keystore: {}", e);
                Ok(EncryptionConfig::default())
            }
        }
    }

    /// Save encryption configuration to OS keystore
    pub fn save(&self) -> Result<()> {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
        let config_json = serde_json::to_string(self)?;
        
        entry.set_password(&config_json)
            .map_err(|e| anyhow::anyhow!("Failed to save encryption config to keystore: {}", e))?;
        
        Ok(())
    }

    /// Set password and update configuration
    pub fn set_password(&mut self, password: &str) -> Result<()> {
        let salt = DatabaseEncryption::generate_salt();
        let password_hash = DatabaseEncryption::hash_password(password, &salt);
        
        self.password_hash = Some(password_hash);
        self.salt = Some(salt);
        self.enabled = true;
        self.database_encrypted = true;
        
        self.save()?;
        Ok(())
    }

    /// Verify a password against stored hash
    pub fn verify_password(&self, password: &str) -> bool {
        if let (Some(stored_hash), Some(salt)) = (&self.password_hash, &self.salt) {
            DatabaseEncryption::verify_password(password, salt, stored_hash)
        } else {
            false
        }
    }

    /// Check if encryption is enabled and password is set
    pub fn is_encryption_ready(&self) -> bool {
        self.enabled && self.password_hash.is_some() && self.salt.is_some()
    }

    /// Check if database is currently encrypted
    pub fn is_database_encrypted(&self) -> bool {
        self.database_encrypted
    }

    /// Disable encryption (for migration from encrypted to unencrypted)
    pub fn disable_encryption(&mut self) -> Result<()> {
        self.enabled = false;
        self.password_hash = None;
        self.salt = None;
        self.database_encrypted = false;
        
        self.save()?;
        Ok(())
    }

    /// Re-enable encryption configuration (without setting password)
    /// This puts the system back into the "configured but no password" state
    pub fn re_enable_encryption(&mut self) -> Result<()> {
        self.enabled = true;
        self.password_hash = None;
        self.salt = None;
        self.database_encrypted = false;
        
        self.save()?;
        Ok(())
    }

    /// Get salt for database encryption initialization
    pub fn get_salt(&self) -> Option<&String> {
        self.salt.as_ref()
    }

    /// Get password hash for verification
    pub fn get_password_hash(&self) -> Option<&String> {
        self.password_hash.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_config_default() {
        let config = EncryptionConfig::default();
        assert!(config.enabled);
        assert!(!config.is_encryption_ready());
        assert!(!config.is_database_encrypted());
    }

    #[test]
    fn test_password_setting_and_verification() {
        let mut config = EncryptionConfig::default();
        let password = "test_password_123";
        
        // Set password
        config.set_password(password).unwrap();
        assert!(config.is_encryption_ready());
        assert!(config.is_database_encrypted());
        
        // Verify password
        assert!(config.verify_password(password));
        assert!(!config.verify_password("wrong_password"));
    }
}