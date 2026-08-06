//! Error types for the DLNA library

use std::path::PathBuf;

/// Main error type for the DLNA library
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(String),

    #[error("SSDP error: {0}")]
    Ssdp(String),

    #[error("UPnP error: {0}")]
    Upnp(String),

    #[error("XML parsing error: {0}")]
    Xml(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Timeout")]
    Timeout,

    #[error("Invalid range: {0}")]
    InvalidRange(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Unsupported: {0}")]
    Unsupported(String),
}

impl From<rupnp::Error> for Error {
    fn from(e: rupnp::Error) -> Self {
        Error::Upnp(e.to_string())
    }
}

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Error::Network(format!("URL parse error: {e}"))
    }
}

impl From<quick_xml::Error> for Error {
    fn from(e: quick_xml::Error) -> Self {
        Error::Xml(e.to_string())
    }
}

/// Result type alias using our Error type
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Returns a user-friendly error message suitable for UI display
    pub fn user_friendly_message(&self) -> String {
        match self {
            Error::Io(_) => "Ошибка ввода/вывода. Проверьте доступ к файлам.".into(),
            Error::Network(_) => "Ошибка сети. Проверьте подключение.".into(),
            Error::Ssdp(_) => "Ошибка обнаружения устройств.".into(),
            Error::Upnp(_) => "Ошибка протокола UPnP.".into(),
            Error::Xml(_) => "Ошибка обработки ответа устройства.".into(),
            Error::Http(_) => "Ошибка HTTP соединения.".into(),
            Error::DeviceNotFound(_) => "Устройство не найдено в сети.".into(),
            Error::ServiceNotFound(_) => "Сервис не поддерживается устройством.".into(),
            Error::FileNotFound(_) => "Файл не найден.".into(),
            Error::InvalidResponse(_) => "Некорректный ответ устройства.".into(),
            Error::Timeout => "Превышено время ожидания ответа.".into(),
            Error::InvalidRange(_) => "Некорректный диапазон для потоковой передачи.".into(),
            Error::Config(_) => "Ошибка конфигурации.".into(),
            Error::Unsupported(_) => "Устройство не поддерживает эту функцию.".into(),
        }
    }

    /// Returns a list of suggested recovery actions
    pub fn recovery_actions(&self) -> Vec<&'static str> {
        match self {
            Error::Network(_) | Error::Ssdp(_) => vec![
                "Проверить Wi-Fi подключение",
                "Убедиться что устройство в той же сети",
                "Попробовать снова",
            ],
            Error::DeviceNotFound(_) => vec![
                "Повторить поиск устройств",
                "Проверить включено ли устройство",
                "Проверить сетевое подключение устройства",
            ],
            Error::Timeout => vec![
                "Попробовать снова",
                "Проверить доступность устройства",
                "Увеличить таймаут",
            ],
            Error::FileNotFound(_) => {
                vec!["Проверить путь к файлу", "Убедиться что файл существует"]
            }
            Error::ServiceNotFound(_) => vec![
                "Проверить что устройство поддерживает DLNA",
                "Попробовать другое устройство",
            ],
            Error::Unsupported(_) => vec!["Попробовать другой протокол или устройство"],
            _ => vec!["Попробовать снова"],
        }
    }

    /// Returns whether the error is recoverable (can be retried)
    pub fn is_recoverable(&self) -> bool {
        match self {
            Error::Timeout | Error::Network(_) | Error::Ssdp(_) | Error::Http(_) => true,
            Error::Io(e) => matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::WouldBlock
            ),
            _ => false,
        }
    }
}
