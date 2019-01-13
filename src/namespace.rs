use std::env;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path;
use std::process;

use failure::err_msg;
use failure::Error;
use failure::ResultExt;
use std::ffi::CString;

#[derive(Debug)]
pub struct Child {
    send: os_pipe::PipeWriter,
    recv: os_pipe::PipeReader,
    pid: nix::unistd::Pid,
}

impl Child {
    pub fn wait(self) -> Result<i32, Error> {
        use nix::sys::wait::*;
        match waitpid(self.pid, None)? {
            WaitStatus::Exited(_, status) => Ok(status),
            status => Err(format_err!("{:?}", status)),
        }
    }
}

pub fn prepare(distro: &str) -> Result<Child, Error> {
    let root = format!("{}/root", distro);

    // TODO: do we need to do this unconditionally?
    if !path::Path::new(&root).is_dir() {
        fs::create_dir(&root)?;
        crate::unpack::unpack(&format!("{}/amd64-root.tar.gz", distro), &root)?;
    }

    let (mut from_recv, from_send) = os_pipe::pipe()?;
    let (into_recv, into_send) = os_pipe::pipe()?;

    {
        use std::os::unix::fs::PermissionsExt;
        let finit_host = format!("{}/bin/finit", root);
        fs::write(&finit_host, &include_bytes!("../target/debug/finit")[..])?;
        fs::set_permissions(&finit_host, fs::Permissions::from_mode(0o755))?;
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

    let mut buf = [0u8; 16];
    from_recv
        .read_exact(&mut buf)
        .with_context(|_| err_msg("reading message header"))?;
    println!("{:?}", buf);

    Ok(Child {
        recv: from_recv,
        send: into_send,
        pid: first_fork,
    })
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
    send: os_pipe::PipeWriter,
) -> Result<void::Void, Error> {
    use nix::unistd::*;

    close_stdin()?;

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
        ForkResult::Parent { child: _ } => {
            use nix::sys::wait::*;
            // Mmm, not sure this is useful or even helpful.
            process::exit(match wait()? {
                WaitStatus::Exited(_, code) => code,
                _ => 66,
            });
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

fn setup_pid_1(recv: os_pipe::PipeReader, send: os_pipe::PipeWriter) -> Result<void::Void, Error> {
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

    let recv = dup(recv.as_raw_fd())?;
    let send = dup(send.as_raw_fd())?;

    let proc = CString::new("/bin/finit")?;
    let argv0 = proc.clone();
    let recv = CString::new(format!("{}", recv))?;
    let send = CString::new(format!("{}", send))?;

    void::unreachable(execv(&proc, &[argv0, recv, send]).with_context(|_| err_msg("exec finit"))?);
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
