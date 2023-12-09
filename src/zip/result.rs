use std::io;
use thiserror::Error;

pub type ZipResult<T> = Result<T, ZipError>;

#[derive(Error, Debug)]
pub enum ZipError {
    #[error("{}", .0)]
    Io(#[from] io::Error),

    #[error("{}", .0)]
    InvalidArchive(&'static str),

    #[error("Support for multi - disk files is not implemented")]
    UnsupportedArchive,

    #[error("AES extra data field has an unsupported length")]
    UnsupportedAesExtraData,

    #[error("UnsupportedCompressionMethod {}", .0)]
    UnsupportedCompressionMethod(u16),

    #[error("FileNotFound")]
    FileNotFound,
}