use std::net::SocketAddrV4;

#[non_exhaustive]
#[derive(Default)]
pub struct Conf {
    /// lightningd command line arguments containing no spaces like `vec!["--rgb=AABBCC", "-regtest"]`
    /// note that `--lightning-dir=<dir>`, `--network+regtest`
    /// cannot be used because they are automatically initialized.
    pub args: Vec<String>,

    /// if `true` bitcoind log output will not be suppressed
    pub view_stdout: bool,

    /// Allows to specify options to open p2p port or connect to the another node
    pub p2p: P2P,
}

/// Enum to specify p2p settings
#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct P2P {
    pub connect: Option<IdHost>, // available only if the node is listening
    pub listen_announce: ListenAnnounce,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct IdHost {
    pub id: String,
    pub host: Option<SocketAddrV4>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ListenAnnounce {
    No, // default
    Listen,
    ListenAndAnnounce,
}

impl Default for ListenAnnounce {
    fn default() -> Self {
        ListenAnnounce::No
    }
}
