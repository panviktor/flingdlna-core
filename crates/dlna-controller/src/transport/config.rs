use std::time::Duration;

/// Configuration for SOAP requests with retry and timeout
#[derive(Debug, Clone)]
pub struct SoapConfig {
    /// Timeout for each SOAP request
    pub timeout: Duration,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay between retries (exponential backoff)
    pub base_delay: Duration,
}

impl Default for SoapConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            max_retries: 3,
            base_delay: Duration::from_millis(500),
        }
    }
}

/// Global SOAP config - can be modified via set_soap_config
static SOAP_CONFIG: std::sync::OnceLock<SoapConfig> = std::sync::OnceLock::new();

/// Set the global SOAP configuration
pub fn set_soap_config(config: SoapConfig) {
    let _ = SOAP_CONFIG.set(config);
}

/// Get the current SOAP configuration
pub(crate) fn get_soap_config() -> &'static SoapConfig {
    SOAP_CONFIG.get_or_init(SoapConfig::default)
}
