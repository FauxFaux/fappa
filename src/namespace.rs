use std::env;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::mem;
use std::net::Ipv6Addr;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::RawFd;
use std::os::unix::process::CommandExt;
use std::path;
use std::process;

use failure::err_msg;
use failure::Error;
use failure::ResultExt;
use nix::unistd::Gid;
use nix::unistd::Uid;
use rand::Rng;
use std::ffi::CString;

pub fn prepare(distro: &str) -> Result<process::Child, Error> {
    let root = format!("{}/root", distro);

    // TODO: do we need to do this unconditionally?
    if !path::Path::new(&root).is_dir() {
        fs::create_dir(&root)?;
        crate::unpack::unpack(&format!("{}/amd64-root.tar.gz", distro), &root)?;
    }

    let (mut from_recv, mut from_send) = os_pipe::pipe()?;
    let (mut into_recv, mut into_send) = os_pipe::pipe()?;

    {
        use std::os::unix::fs::PermissionsExt;
        let finit_host = format!("{}/bin/finit", root);
        fs::write(&finit_host, &include_bytes!("../target/debug/finit")[..])?;
        let mut initial = fs::File::open(&finit_host)?.metadata()?.permissions();
        initial.set_mode(0o755);
        fs::set_permissions(&finit_host, initial)?;
    }

    let first_fork = {
        use nix::unistd::*;
        match fork()? {
            ForkResult::Parent { child } => child,
            ForkResult::Child => {
                drop(from_recv);
                drop(into_send);

                close_stdin()?;

                from_send.write_all(&[0x01]).expect("write");

                inside(&root).expect("child setup");

                from_send.write_all(&[0x02]).expect("write");

                match fork()? {
                    ForkResult::Parent { child } => {
                        println!("inner fork: {:?}", child);
                        process::exit(69);
                    }

                    ForkResult::Child => {
                        from_send.write_all(&[0x03]).expect("write");
                        println!("inner child actually: {:?}", getpid());
                        let sh = CString::new("/bin/finit")?;
                        from_send.write_all(&[0x04]).expect("write");
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                        execv(&sh.clone(), &[sh]).expect("exec");
                        process::exit(71);
                    }
                }
            }
        }
    };

    drop(into_recv);
    drop(from_send);

    let mut buf = [0u8; 4];
    from_recv.read_exact(&mut buf)?;
    println!("{:?}", buf);

    Ok(unimplemented!())
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
            nix::sys::stat::Mode::empty(),
        )?
    );

    Ok(())
}

fn inside(root: &str) -> Result<(), Error> {
    let real_euid = nix::unistd::geteuid();
    let real_egid = nix::unistd::getegid();

    {
        use nix::sched::*;
        unshare(
            CloneFlags::CLONE_NEWIPC
                | CloneFlags::CLONE_NEWNS
                | CloneFlags::CLONE_NEWPID
                | CloneFlags::CLONE_NEWUSER
                | CloneFlags::CLONE_NEWUTS,
        )
        .with_context(|_| err_msg("unshare"))?;
    }

    {
        let unset: Option<&str> = None;
        use nix::mount::*;

        mount(
            Some("none"),
            "/",
            unset,
            MsFlags::MS_REC | MsFlags::MS_PRIVATE,
            unset,
        )?;

        mount(
            Some(root),
            root,
            unset,
            MsFlags::MS_BIND | MsFlags::MS_NOSUID,
            unset,
        )?;

        env::set_current_dir(root)?;

        mount(
            Some("/proc"),
            "proc",
            unset,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            unset,
        )?;

        mount(
            Some("/sys"),
            "sys",
            unset,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            unset,
        )?;
    }

    fs::OpenOptions::new()
        .write(true)
        .open("/proc/self/uid_map")?
        .write_all(format!("0 {} 1", real_euid).as_bytes())?;

    drop_setgroups()?;

    fs::OpenOptions::new()
        .write(true)
        .open("/proc/self/gid_map")?
        .write_all(format!("0 {} 1", real_egid).as_bytes())?;

    nix::unistd::setresuid(Uid::from_raw(0), Uid::from_raw(0), Uid::from_raw(0))?;
    nix::unistd::setresgid(Gid::from_raw(0), Gid::from_raw(0), Gid::from_raw(0))?;

    fs::remove_dir("old")?;
    fs::create_dir("old")?;
    nix::unistd::pivot_root(&Some("."), &Some("old"))?;

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

    fn into_inner(mut self) -> RawFd {
        let tmp = self.fd;
        // stop us from dropping ourselves. :|
        self.fd = -1;
        tmp
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        self.close().expect("closing during drop")
    }
}
