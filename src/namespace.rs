use std::fs;
use std::io;
use std::io::Write;
use std::mem;
use std::net::Ipv6Addr;
use std::os::unix::io::RawFd;
use std::os::unix::process::CommandExt;
use std::process;

use failure::err_msg;
use failure::Error;
use failure::ResultExt;
use rand::Rng;

pub fn prepare() -> Result<process::Child, Error> {
    use nix::sys::socket::*;

    let (to_namespace, to_host) = socketpair(
        AddressFamily::Unix,
        SockType::Datagram,
        None,
        SockFlag::empty(),
    )
    .with_context(|_| err_msg("creating socket pair"))?;

    let to_namespace = OwnedFd::new(to_namespace);
    let to_host = OwnedFd::new(to_host);

    let child = {
        let child_to_host = to_host.fd;
        let child_to_namespace = to_namespace.fd;
        process::Command::new("/bin/bash")
            .before_exec(move || {
                mem::drop(OwnedFd::new(child_to_namespace));
                let to_host = OwnedFd::new(child_to_host);
                inside().expect("really should work out how to pass this");
                Ok(())
            })
            .spawn()?
    };

    close_stdin()?;
    mem::drop(to_host);

    // .. child actually sends something...

    mem::drop(to_namespace);

    Ok(child)
}

/// Super dodgy reopen here; should re-do freopen?
fn close_stdin() -> Result<(), Error> {
    nix::unistd::close(0)?;

    use nix::fcntl::*;
    // Third argument ignored, as we're not creating the file.
    assert_eq!(
        0,
        open(
            "/dev/null",
            OFlag::O_RDONLY | OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::S_IRUSR,
        )?
    );

    Ok(())
}

fn ula_zero() -> Ipv6Addr {
    let mut bytes = [0u8; 16];
    bytes[0] = 0xfd;
    bytes[1..6].copy_from_slice(&rand::thread_rng().gen::<[u8; 5]>());
    bytes.into()
}

fn inside() -> Result<(), Error> {
    let real_euid = nix::unistd::geteuid();
    let real_egid = nix::unistd::getegid();

    {
        use nix::sched::*;
        unshare(CloneFlags::CLONE_NEWNET | CloneFlags::CLONE_NEWUSER)
            .with_context(|_| err_msg("unsharing"))?;
    }

    if true {
        drop_setgroups()?;

        fs::OpenOptions::new()
            .write(true)
            .open("/proc/self/uid_map")?
            .write_all(format!("0 {} 1", real_euid).as_bytes())?;

        fs::OpenOptions::new()
            .write(true)
            .open("/proc/self/gid_map")?
            .write_all(format!("0 {} 1", real_egid).as_bytes())?;
    }

    Ok(())
}

fn drop_setgroups() -> Result<(), Error> {
    match fs::OpenOptions::new()
        .write(true)
        .open("/proc/self/setgroups")
    {
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
            // Maybe the system doesn't care?
            Ok(())
        }
        Ok(mut file) => {
            file.write_all(b"deny")?;
            Ok(())
        }
        Err(e) => Err(e).with_context(|_| err_msg("unknown error opening setgroups"))?,
    }
}

pub struct OwnedFd {
    pub fd: RawFd,
}

impl OwnedFd {
    pub fn new(fd: RawFd) -> Self {
        OwnedFd { fd }
    }

    fn close(&mut self) -> Result<(), Error> {
        if -1 == self.fd {
            return Ok(());
        }
        nix::unistd::close(self.fd)?;
        self.fd = -1;
        Ok(())
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        self.close().expect("closing during drop")
    }
}
