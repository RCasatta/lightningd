#[derive(Debug)]
pub enum Error {
    /// Wrapper of io Error
    Io(std::io::Error),

    /// Wrapper of rpc client Error
    Rpc(clightningrpc::Error),

    SockPathNotExist,

    GetInfoSyncing,
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<clightningrpc::Error> for Error {
    fn from(e: clightningrpc::Error) -> Self {
        Error::Rpc(e)
    }
}
