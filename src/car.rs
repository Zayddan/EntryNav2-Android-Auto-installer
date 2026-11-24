//! SSH connection to the head unit (EntryNav2 / EntryEvo).

use anyhow::{anyhow, bail, Context, Result};
use ssh2::{HashType, Session};
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

#[derive(Clone)]
pub struct ConnParams {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

impl ConnParams {
    pub fn id(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

// Never print the password, even in debug output.
impl fmt::Debug for ConnParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnParams")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .finish()
    }
}

pub struct Car {
    sess: Session,
    pub fingerprint: String,
}

impl Car {
    /// Connects and authenticates. The host key is not pinned (physical access is
    /// required); its fingerprint is logged.
    pub fn connect(p: &ConnParams) -> Result<Self> {
        let addr = (p.host.as_str(), p.port)
            .to_socket_addrs()
            .with_context(|| format!("invalid address: {}", p.host))?
            .next()
            .ok_or_else(|| anyhow!("address not found: {}", p.host))?;
        let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(8)).with_context(|| {
            format!("cannot reach {addr} (ENET cable plugged in? ignition on?)")
        })?;

        let mut sess = Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.set_timeout(20_000);
        sess.handshake().context("SSH handshake failed")?;

        let fingerprint = sess
            .host_key_hash(HashType::Sha256)
            .map(|h| {
                h.iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(":")
            })
            .ok_or_else(|| anyhow!("the head unit did not send a host key"))?;

        sess.userauth_password(&p.user, &p.password)
            .map_err(|_| anyhow!("authentication refused (wrong user or password)"))?;
        if !sess.authenticated() {
            bail!("authentication refused");
        }
        // The compatibility test can stay silent for up to 60 s.
        sess.set_timeout(180_000);
        Ok(Self { sess, fingerprint })
    }

    /// Runs a script and passes each output line as it arrives. Returns the exit code.
    pub fn run(&self, script: &str, mut on_line: impl FnMut(&str)) -> Result<i32> {
        let mut ch = self.sess.channel_session()?;
        ch.exec(script)?;
        ch.send_eof()?;

        let mut buf = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            let n = ch.read(&mut buf)?;
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buf[..n]);
            while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = pending.drain(..=pos).collect();
                on_line(String::from_utf8_lossy(&line).trim_end());
            }
        }
        if !pending.is_empty() {
            on_line(String::from_utf8_lossy(&pending).trim_end());
        }
        let mut err = String::new();
        let _ = ch.stderr().read_to_string(&mut err);
        for l in err.lines() {
            on_line(l);
        }
        ch.wait_close()?;
        Ok(ch.exit_status()?)
    }

    /// Runs a script and returns all output lines.
    pub fn run_collect(&self, script: &str) -> Result<(i32, Vec<String>)> {
        let mut lines = Vec::new();
        let rc = self.run(script, |l| lines.push(l.to_string()))?;
        Ok((rc, lines))
    }

    /// Uploads with `cat > file` (no scp/sftp on the car), then checks the size.
    pub fn upload(
        &self,
        local: &Path,
        remote: &str,
        mut progress: impl FnMut(u64, u64) -> bool,
    ) -> Result<()> {
        let total = std::fs::metadata(local)?.len();
        let mut file = File::open(local)?;
        let mut ch = self.sess.channel_session()?;
        ch.exec(&format!("cat > {}", crate::scripts::sh_quote(remote)))?;

        let mut buf = vec![0u8; 32 * 1024];
        let mut done = 0u64;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            ch.write_all(&buf[..n])?;
            done += n as u64;
            if !progress(done, total) {
                let _ = ch.close();
                return Err(crate::worker::Cancelled.into());
            }
        }
        ch.flush()?;
        ch.send_eof()?;
        ch.wait_eof()?;
        ch.wait_close()?;
        if ch.exit_status()? != 0 {
            bail!("writing {remote} failed (not enough space in /tmp?)");
        }

        let (_, out) =
            self.run_collect(&format!("wc -c < {}", crate::scripts::sh_quote(remote)))?;
        let remote_size: u64 = out
            .iter()
            .find_map(|l| l.trim().parse().ok())
            .ok_or_else(|| anyhow!("could not read the size of the uploaded file"))?;
        if remote_size != total {
            bail!("incomplete upload ({remote_size} / {total} bytes)");
        }
        Ok(())
    }

    /// Starts a reboot; the head unit closes the connection.
    pub fn reboot(&self) {
        if let Ok(mut ch) = self.sess.channel_session() {
            let _ = ch.exec("sync; reboot");
            let _ = ch.send_eof();
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
