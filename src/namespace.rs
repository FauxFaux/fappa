use std::env;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::mem;
use std::net::Ipv6Addr;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
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
            ForkResult::Child => match setup_namespace(&root, into_recv, from_send) {
                Ok(v) => void::unreachable(v),
                Err(e) => {
                    eprintln!("sandbox setup failed: {:?}", e);
                    process::exit(67);
                }
            },
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

fn setup_namespace(
    root: &str,
    recv: os_pipe::PipeReader,
    mut send: os_pipe::PipeWriter,
) -> Result<void::Void, Error> {
    use nix::unistd::*;

    close_stdin()?;

    send.write_all(&[0x01])?;

    let real_euid = geteuid();
    let real_egid = getegid();

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
        )
        .with_context(|_| err_msg("mount --make-rprivate"))?;

        // mount our unpacked root on itself, inside the new namespace
        mount(
            Some(root),
            root,
            unset,
            MsFlags::MS_BIND | MsFlags::MS_NOSUID,
            unset,
        )
        .with_context(|_| err_msg("mount $root $root"))?;

        env::set_current_dir(root)?;

        // make /proc visible inside the chroot.
        // without this, `mount -t proc proc /proc` fails with EPERM.
        // No, I don't know where this is documented.
        make_mount_destination(".host-proc")?;
        mount(
            Some("/proc"),
            ".host-proc",
            unset,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            unset,
        )
        .with_context(|_| err_msg("mount --bind /proc .host-proc"))?;

        mount(
            Some("/sys"),
            "sys",
            unset,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            unset,
        )
        .with_context(|_| err_msg("mount --bind /sys sys"))?;
    }

    fs::OpenOptions::new()
        .write(true)
        .open("/proc/self/uid_map")
        .with_context(|_| err_msg("host uid_map"))?
        .write_all(format!("0 {} 1", real_euid).as_bytes())
        .with_context(|_| err_msg("writing uid_map"))?;

    drop_setgroups().with_context(|_| err_msg("drop_setgroups"))?;

    fs::OpenOptions::new()
        .write(true)
        .open("/proc/self/gid_map")
        .with_context(|_| err_msg("host gid_map"))?
        .write_all(format!("0 {} 1", real_egid).as_bytes())
        .with_context(|_| err_msg("writing gid_map"))?;

    setresuid(Uid::from_raw(0), Uid::from_raw(0), Uid::from_raw(0))
        .with_context(|_| err_msg("setuid"))?;
    setresgid(Gid::from_raw(0), Gid::from_raw(0), Gid::from_raw(0))
        .with_context(|_| err_msg("setgid"))?;

    make_mount_destination("old")?;
    pivot_root(&Some("."), &Some("old")).with_context(|_| err_msg("pivot_root"))?;
    nix::mount::umount2("old", nix::mount::MntFlags::MNT_DETACH)
        .with_context(|_| err_msg("unmount old"))?;
    fs::remove_dir("old").with_context(|_| err_msg("rm old"))?;

    match fork()? {
        ForkResult::Parent { child } => {
            println!("inner fork: {:?}", child);
            process::exit(69);
        }

        ForkResult::Child => match setup_pid_1(recv, send) {
            Ok(v) => void::unreachable(v),
            Err(e) => {
                eprintln!("sandbox setup pid1 failed: {:?}", e);
                process::exit(67);
            }
        },
    }
}

fn setup_pid_1(
    recv: os_pipe::PipeReader,
    mut send: os_pipe::PipeWriter,
) -> Result<void::Void, Error> {
    use nix::unistd::*;

    println!("inner child actually: {:?}", getpid());

    {
        let unset: Option<&str> = None;
        use nix::mount::*;

        mount(
            Some("/proc"),
            "/proc",
            Some("proc"),
            MsFlags::MS_NOSUID,
            unset,
        )
        .with_context(|_| err_msg("mount proc -t proc /proc"))?;

        umount2(".host-proc", MntFlags::MNT_DETACH)
            .with_context(|_| err_msg("unmount .host-proc"))?;

        fs::remove_dir(".host-proc")?;

        mount(
            Some("/"),
            "/",
            unset,
            MsFlags::MS_RDONLY | MsFlags::MS_BIND | MsFlags::MS_NOSUID | MsFlags::MS_REMOUNT,
            unset,
        )
        .with_context(|_| err_msg("finalising /"))?;
    }

    let proc = CString::new("/bin/finit")?;

    set_cloexec(send.as_raw_fd(), false)?;
    set_cloexec(recv.as_raw_fd(), false)?;
    dup(send.as_raw_fd())?;
    dup(recv.as_raw_fd())?;
    void::unreachable(execv(&proc.clone(), &[proc]).with_context(|_| err_msg("exec finit"))?);
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
            file.write_all(b"deny")
                .with_context(|_| err_msg("writing setgroups"))?;
            Ok(())
        }
        Err(e) => Err(e).with_context(|_| err_msg("unknown error opening setgroups"))?,
    }
}

fn make_mount_destination(name: &'static str) -> Result<(), Error> {
    let _ = fs::remove_dir(name);
    fs::create_dir(name)
        .with_context(|_| format_err!("creating {} before mounting on it", name))?;
    fs::set_permissions(name, fs::Permissions::from_mode(0o644))?;
    Ok(())
}

fn set_cloexec(fd: RawFd, on: bool) -> Result<(), Error> {
    use nix::fcntl::*;
    let mut current = OFlag::from_bits(fcntl(fd, FcntlArg::F_GETFL)?)
        .ok_or_else(|| err_msg("unrecognised fcntl bits"))?;

    if on {
        current.insert(OFlag::O_CLOEXEC);
    } else {
        current.remove(OFlag::O_CLOEXEC);
    }

    fcntl(fd, FcntlArg::F_SETFL(current))?;

    Ok(())
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
