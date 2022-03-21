use std::{
    ffi::OsStr,
    io::Read,
    net::TcpListener,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use bitcoind::BitcoinD;
use clightningrpc::LightningRPC;
use log::debug;
use tempfile::TempDir;

/// Struct representing the bitcoind process with related information
pub struct LightningD {
    /// Process child handle, used to terminate the process when this struct is dropped
    process: Child,
    /// Rpc client linked to this bitcoind process
    pub client: LightningRPC,
    /// Work directory, where the node store blocks and other stuff. It is kept in the struct so that
    /// directory is deleted only when this struct is dropped
    _work_dir: TempDir,
}

#[derive(Debug)]
pub enum Error {
    /// Wrapper of io Error
    Io(std::io::Error),
}

#[derive(Default)]
pub struct Conf {
    /// lightningd command line arguments containing no spaces like `vec!["--rgb=AABBCC", "-regtest"]`
    /// note that `--lightning-dir=<dir>`, `--network+regtest`
    /// cannot be used because they are automatically initialized.
    pub args: Vec<String>,

    /// if `true` bitcoind log output will not be suppressed
    pub view_stdout: bool,
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
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

        let mut cookie = std::fs::File::open(&bitcoind.params.cookie_file)?;
        let mut cookie_value = String::new();
        cookie.read_to_string(&mut cookie_value)?;
        debug!("cookie file: ({})", cookie_value);
        let values: Vec<&str> = cookie_value.split(':').collect();

        let rpcuser = format!("--bitcoin-rpcuser={}", values[0]);
        let rpcpassword = format!("--bitcoin-rpcpassword={}", values[1]);

        let lightning_dir_arg = format!("--lightning-dir={}", temp_path.display());
        let process = Command::new(exe.as_ref())
            .arg("--network=regtest")
            .arg(rpcconnect)
            .arg(rpcport)
            .arg(rpcuser)
            .arg(rpcpassword)
            .arg(lightning_dir_arg)
            .stdout(stdout)
            .spawn()?;

        let mut sock_path = temp_path.to_path_buf();
        sock_path.push("regtest");
        sock_path.push("lightning-rpc");

        for _ in 0..60 {
            if sock_path.exists() {
                break;
            } else {
                thread::sleep(Duration::from_millis(500));
            }
        }

        let client = LightningRPC::new(&sock_path);

        Ok(LightningD {
            process,
            client,
            _work_dir: temp_dir,
        })
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
    use log::log_enabled;
    use log::Level;

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
        assert_eq!(getinfo.blockheight, 100);
    }

    #[test]
    fn two_lightningd() {
        let bitcoind = init();

        let exe = std::env::var("LIGHTNINGD_EXE")
            .expect("LIGHTNINGD_EXE env var pointing to `lightningd` executable is required");

        let mut conf = Conf::default();
        conf.view_stdout = log_enabled!(Level::Debug);

        let _lightningd_1 = LightningD::with_conf(&exe, &bitcoind, &conf).unwrap();
        let _lightningd_2 = LightningD::with_conf(&exe, &bitcoind, &conf).unwrap();
    }

    fn init() -> BitcoinD {
        let _ = env_logger::try_init();
        let bitcoind_exe = exe_path().unwrap();
        let bitcoind = BitcoinD::new(bitcoind_exe).unwrap();
        let address = bitcoind.client.get_new_address(None, None).unwrap();
        bitcoind.client.generate_to_address(100, &address).unwrap();
        bitcoind
    }
}
