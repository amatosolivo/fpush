use derive_more::{Display, From};
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, From, Display)]
pub enum Error {
    Io(std::io::Error),
    ConfigError(serde_json::Error),
    Config(String),
    Xmpp(Box<tokio_xmpp::Error>),
    PubSubNonPublish,
    PubSubInvalidFormat,
    PubSubToManyPublishOptions,
    PubSubInvalidPushModuleConfiguration,
    InvalidPubSubType,
    InvalidNotificationString,
    InvalidNotificationFormat,
}

impl std::convert::From<tokio_xmpp::Error> for Error {
    fn from(e: tokio_xmpp::Error) -> Self {
        Error::Xmpp(Box::new(e))
    }
}
