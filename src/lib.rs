use std::{
    ffi::OsStr,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use bitcoind::BitcoinD;
use clightningrpc::LightningRPC;
pub use conf::Conf;
pub use error::Error;
use log::debug;
use tempfile::TempDir;

use crate::conf::{IdHost, ListenAnnounce};

mod conf;
mod error;

/// Struct representing the bitcoind process with related information
pub struct LightningD {
    /// Process child handle, used to terminate the process when this struct is dropped
    process: Child,
    /// Rpc client linked to this bitcoind process
    pub client: LightningRPC,
    /// Work directory, where the node store blocks and other stuff. It is kept in the struct so that
    /// directory is deleted only when this struct is dropped
    _work_dir: TempDir,

    id_host: Option<IdHost>,
}

impl LightningD {
    /// Launch the bitcoind process from the given `exe` executable with default args.
    ///
    /// Waits for the node to be ready to accept connections before returning
    pub fn new<S: AsRef<OsStr>>(exe: S, bitcoind: &BitcoinD) -> Result<Self, Error> {
        let conf = Conf::default();
        Self::with_conf(exe, bitcoind, &conf)
    }

    /// Create a new electrs process using given [Conf] connected with the given bitcoind
    pub fn with_conf<S: AsRef<OsStr>>(
        exe: S,
        bitcoind: &BitcoinD,
        conf: &Conf,
    ) -> Result<Self, Error> {
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();

        debug!("temp_path: {}", temp_path.display());

        let stdout = if conf.view_stdout {
            Stdio::inherit()
        } else {
            Stdio::null()
        };

        let rpcconnect = format!("--bitcoin-rpcconnect={}", bitcoind.params.rpc_socket.ip());
        let rpcport = format!("--bitcoin-rpcport={}", bitcoind.params.rpc_socket.port());

        let cookie = bitcoind
            .params
            .get_cookie_values()?
            .ok_or(Error::MissingAuth)?;

        let rpcuser = format!("--bitcoin-rpcuser={}", cookie.user);
        let rpcpassword = format!("--bitcoin-rpcpassword={}", cookie.password);

        let lightning_dir_arg = format!("--lightning-dir={}", temp_path.display());

        let mut p2p_args = vec![];
        let listen_on = match conf.p2p.listen_announce {
            ListenAnnounce::No => None,
            ListenAnnounce::Listen => {
                let listen_on =
                    SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), get_available_port()?);
                p2p_args.push(format!("--bind-addr={}", listen_on));
                Some(listen_on)
            }
            ListenAnnounce::ListenAndAnnounce => {
                let listen_on =
                    SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), get_available_port()?);
                p2p_args.push(format!("--addr={}", listen_on));
                Some(listen_on)
            }
        };

        let process = Command::new(exe.as_ref())
            .arg("--network=regtest")
            .arg(rpcconnect)
            .arg(rpcport)
            .arg(rpcuser)
            .arg(rpcpassword)
            .arg(lightning_dir_arg)
            .args(p2p_args)
            .stdout(stdout)
            .spawn()?;

        let mut sock_path = temp_path.to_path_buf();
        sock_path.push("regtest");
        sock_path.push("lightning-rpc");

        for i in 0.. {
            if sock_path.exists() {
                break;
            } else if i >= 60 {
                return Err(Error::SockPathNotExist);
            } else {
                thread::sleep(Duration::from_millis(500));
            }
        }

        let client = LightningRPC::new(&sock_path);

        let mut i = 0;
        let id = loop {
            if let Ok(getinfo) = client.getinfo() {
                if getinfo.warning_bitcoind_sync.is_none()
                    && getinfo.warning_lightningd_sync.is_none()
                {
                    break getinfo.id;
                }
            }
            if i >= 60 {
                return Err(Error::GetInfoSyncing);
            }
            i += 1;
            thread::sleep(Duration::from_millis(500));
        };

        if let Some(IdHost { id, host }) = conf.p2p.connect.as_ref() {
            let connect_result = client.connect(id, host.map(|h| h.to_string()).as_deref())?;
            debug!("connect_result: {:?}", connect_result);
        }

        let id_host = listen_on.map(|host| IdHost {
            id,
            host: Some(host),
        });
        Ok(LightningD {
            process,
            client,
            id_host,
            _work_dir: temp_dir,
        })
    }

    pub fn id_host(&self) -> Option<&IdHost> {
        self.id_host.as_ref()
    }
}

impl Drop for LightningD {
    fn drop(&mut self) {
        let _ = self.client.stop();
        let _ = self.process.kill();
    }
}

/// Returns a non-used local port if available.
///
/// Note there is a race condition during the time the method check availability and the caller
pub fn get_available_port() -> Result<u16, Error> {
    // using 0 as port let the system assign a port available
    let t = TcpListener::bind(("127.0.0.1", 0))?; // 0 means the OS choose a free port
    Ok(t.local_addr().map(|s| s.port())?)
}

#[cfg(test)]
mod tests {
    use bitcoind::bitcoincore_rpc::RpcApi;
    use bitcoind::exe_path;
    use bitcoind::BitcoinD;
    use log::debug;
    use log::log_enabled;
    use log::Level;

    use crate::conf::ListenAnnounce;
    use crate::conf::P2P;
    use crate::Conf;
    use crate::LightningD;

    #[test]
    fn one_lightningd() {
        let bitcoind = init();
        let mut conf = Conf::default();
        conf.view_stdout = log_enabled!(Level::Debug);
        let exe = std::env::var("LIGHTNINGD_EXE")
            .expect("LIGHTNINGD_EXE env var pointing to `lightningd` executable is required");
        let lightningd = LightningD::with_conf(exe, &bitcoind, &conf).unwrap();
        let getinfo = lightningd.client.getinfo().unwrap();
        debug!("{:?}", getinfo);
        assert_eq!(getinfo.blockheight, 100);
    }

    #[test]
    fn two_lightningd() {
        let bitcoind = init();

        let exe = std::env::var("LIGHTNINGD_EXE")
            .expect("LIGHTNINGD_EXE env var pointing to `lightningd` executable is required");

        let mut conf = Conf::default();
        conf.view_stdout = log_enabled!(Level::Debug);
        conf.p2p = P2P {
            connect: None,
            listen_announce: ListenAnnounce::Listen,
        };

        let lightningd_1 = LightningD::with_conf(&exe, &bitcoind, &conf).unwrap();
        assert!(lightningd_1.id_host().is_some());

        conf.p2p = P2P {
            connect: lightningd_1.id_host().cloned(),
            listen_announce: ListenAnnounce::Listen,
        };

        let lightningd_2 = LightningD::with_conf(&exe, &bitcoind, &conf).unwrap();
        let list_peers = lightningd_2.client.listpeers(None, None).unwrap();
        assert_eq!(list_peers.peers.len(), 1);
    }

    fn init() -> BitcoinD {
        let _ = env_logger::try_init();
        let bitcoind_exe = exe_path().unwrap();
        let bitcoind = BitcoinD::new(bitcoind_exe).unwrap();
        let address = bitcoind
            .client
            .get_new_address(None, None)
            .unwrap()
            .assume_checked();
        bitcoind.client.generate_to_address(100, &address).unwrap();
        bitcoind
    }
}
