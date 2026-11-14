/// A failed OS call, with a message meant for the user ("This window is no longer open").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

pub type Result<T = ()> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(message: impl Into<String>) -> Error {
        Error(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error(e.to_string())
    }
}
