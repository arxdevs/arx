pub(crate) mod exit {
    pub const SUCCESS: u8 = 0;
    pub const USAGE: u8 = 64;
    pub const UNAUTHORIZED: u8 = 65;
    pub const NOT_FOUND: u8 = 66;
    pub const ALREADY_EXISTS: u8 = 67;
    pub const BAD_REQUEST: u8 = 68;
    pub const SERVER_ERROR: u8 = 70;
    pub const NETWORK: u8 = 71;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CliError {
    #[error("usage: {0}")]
    Usage(String),
    #[error("unauthorized — run `arx login` (or `arx setup` on a fresh daemon)")]
    Unauthorized,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("network: {0}")]
    Network(String),
    #[error("server: {0}")]
    Server(String),
}

impl CliError {
    pub(crate) fn code(&self) -> u8 {
        match self {
            CliError::Usage(_) => exit::USAGE,
            CliError::Unauthorized => exit::UNAUTHORIZED,
            CliError::NotFound(_) => exit::NOT_FOUND,
            CliError::AlreadyExists(_) => exit::ALREADY_EXISTS,
            CliError::BadRequest(_) => exit::BAD_REQUEST,
            CliError::Network(_) => exit::NETWORK,
            CliError::Server(_) => exit::SERVER_ERROR,
        }
    }
}
