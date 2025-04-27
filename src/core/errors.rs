use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrderError {
    #[error("Invalid order quantity")]
    InvalidQuantity,
    #[error("Invalid order price")]
    InvalidPrice,
    #[error("Channel send error")]
    ChannelError,
}
