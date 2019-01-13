use std::env;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path;
use std::process;

use byteorder::ByteOrder;
use byteorder::WriteBytesExt;
use byteorder::LE;
use cast::u64;
use cast::usize;
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

pub enum FromChild {
    // 1
    Debug(String),
}

impl Child {
    pub fn wait(self) -> Result<i32, Error> {
        use nix::sys::wait::*;
        match waitpid(self.pid, None)? {
            WaitStatus::Exited(_, status) => Ok(status),
            status => Err(format_err!("{:?}", status)),
        }
    }

    pub fn msg(&mut self) -> Result<Option<FromChild>, Error> {
        let (code, data) = self.read_msg()?;
        let ret = match code {
            1 => FromChild::Debug(String::from_utf8(data)?),
            2 => return Ok(None),
            // TODO: should we tell the client to die here?
            code => bail!("unsupported client code: {}", code),
        };
        self.write_msg(0, &[])?;
        Ok(Some(ret))
    }

    fn read_msg(&mut self) -> Result<(u64, Vec<u8>), Error> {
        let mut buf = [0u8; 16];
        self.recv.read_exact(&mut buf)?;
        let len = LE::read_u64(&buf[..=8]);
        let code = LE::read_u64(&buf[8..]);
        let mut buf = vec![0u8; usize(len - 16)];
        self.recv.read_exact(&mut buf)?;
        Ok((code, buf))
    }

    fn write_msg(&mut self, code: u64, data: &[u8]) -> Result<(), Error> {
        let total = 16 + data.len();
        let mut msg = Vec::with_capacity(total);
        // header: length (including header), code
        msg.write_u64::<LE>(u64(total))?;
        msg.write_u64::<LE>(code)?;

        // data:
        msg.extend_from_slice(data);
        self.send.write_all(&msg)?;
        Ok(())
    }
}

pub fn prepare(distro: &str) -> Result<Child, Error> {
    let root = format!("{}/root", distro);

    // TODO: do we need to do this unconditionally?
    if !path::Path::new(&root).is_dir() {
        fs::create_dir(&root)?;
        crate::unpack::unpack(&format!("{}/amd64-root.tar.gz", distro), &root)?;
    }

    let (from_recv, from_send) = os_pipe::pipe()?;
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

    {
        let us = getpid().as_raw();
        ensure!(1 == us, "we failed to actually end up as pid 1: {}", us);
    }

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

    drop_caps()?;

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

fn drop_caps() -> Result<(), Error> {
    // man:capabilities(7)
    //
    // An  application  can use the following call to lock
    // itself, and all of its descendants, into  an  envi‐
    // ronment  where the only way of gaining capabilities
    // is by executing  a  program  with  associated  file
    // capabilities:
    //
    //     prctl(PR_SET_SECUREBITS,
    //          /* SECBIT_KEEP_CAPS off */
    //             SECBIT_KEEP_CAPS_LOCKED |
    //             SECBIT_NO_SETUID_FIXUP |
    //             SECBIT_NO_SETUID_FIXUP_LOCKED |
    //             SECBIT_NOROOT |
    //             SECBIT_NOROOT_LOCKED);
    //             /* Setting/locking SECBIT_NO_CAP_AMBIENT_RAISE
    //                is not required */
    //
    // 0x2f == that value, which isn't currently exposed by libc::.
    unsafe { libc::prctl(libc::PR_SET_SECUREBITS, 0x2f, 0, 0, 0) };

    let max_cap: libc::c_int = fs::read_to_string("/proc/sys/kernel/cap_last_cap")?
        .trim()
        .parse()?;

    ensure!(max_cap > 0, "negative cap? {}", max_cap);

    for cap in 0..=max_cap {
        match unsafe { libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0) } {
            0 | libc::EINVAL => (),
            e => Err(nix::errno::Errno::from_i32(e))?,
        }
    }

    Ok(())
}

fn make_mount_destination(name: &'static str) -> Result<(), Error> {
    let _ = fs::remove_dir(name);
    fs::create_dir(name)
        .with_context(|_| format_err!("creating {} before mounting on it", name))?;
    fs::set_permissions(name, fs::Permissions::from_mode(0o644))?;
    Ok(())
}
