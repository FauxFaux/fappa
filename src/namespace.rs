use std::env;
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::process;

use failure::ensure;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use failure::ResultExt;
use log::error;
use log::info;
use void::ResultVoidErrExt;

pub mod child;
mod id_map;

pub fn unpack_to_temp<P: AsRef<Path>>(cache: P, distro: &str) -> Result<tempfile::TempDir, Error> {
    let mut root = super::fetch_images::base_image(cache, distro)?;
    root.push("root.tar.zstd");

    let temp = tempfile::TempDir::new()?;
    crate::unpack::unpack(&root, &temp)
        .with_context(|_| format_err!("unpacking {:?} to {:?}", root, temp))?;

    Ok(temp)
}

pub fn launch_our_init<P: AsRef<Path>>(root: P) -> Result<child::Child, Error> {
    let (from_recv, from_send) = os_pipe::pipe()?;
    let (into_recv, into_send) = os_pipe::pipe()?;

    {
        let mut finit_host = root.as_ref().to_path_buf();
        finit_host.push("bin");
        finit_host.push("finit");
        reflink::reflink_or_copy("target/x86_64-unknown-linux-musl/debug/finit", &finit_host)
            .with_context(|_| err_msg("copying init from host to child"))?;
        fs::set_permissions(&finit_host, fs::Permissions::from_mode(0o755))?;
        info!("finit written to {:?}", finit_host);

        let mut resolv_conf_host = root.as_ref().to_path_buf();
        resolv_conf_host.push("etc");
        resolv_conf_host.push("resolv.conf");
        fs::write(resolv_conf_host, b"nameserver 127.0.0.53")?;
    }

    let first_fork = {
        use nix::unistd::*;
        match fork()? {
            ForkResult::Parent { child } => child,
            ForkResult::Child => {
                let e = setup_namespace(&root, into_recv, from_send).void_unwrap_err();
                error!("sandbox setup failed: {:?}", e);
                process::exit(67);
            }
        }
    };

    let mut proto = child::Proto {
        recv: from_recv,
        send: into_send,
        _types: Default::default(),
    };

    proto.init_await_map_command()?;

    id_map::map_us(first_fork)?;

    proto.init_map_complete()?;

    Ok(child::Child {
        proto,
        pid: first_fork,
    })
}

fn reopen_stdin_as_null() -> Result<(), Error> {
    nix::unistd::dup3(
        fs::File::open("/dev/null")?.as_raw_fd(),
        0,
        nix::fcntl::OFlag::empty(),
    )?;

    Ok(())
}

fn setup_namespace<P: AsRef<Path>>(
    root: P,
    mut recv: os_pipe::PipeReader,
    mut send: os_pipe::PipeWriter,
) -> Result<void::Void, Error> {
    use nix::unistd::*;

    reopen_stdin_as_null()?;

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
            Some(root.as_ref()),
            root.as_ref(),
            unset,
            MsFlags::MS_BIND | MsFlags::MS_NOSUID,
            unset,
        )
        .with_context(|_| err_msg("mount $root $root"))?;

        env::set_current_dir(&root)?;

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

        drop(fs::File::create("dev/null")?);
        mount(
            Some("/dev/null"),
            "dev/null",
            unset,
            MsFlags::MS_BIND,
            unset,
        )
        .with_context(|_| err_msg("mount --bind /dev/null"))?;
    }

    child::Proto::<u64, u64>::await_maps(&mut send, &mut recv)?;

    setresuid(Uid::from_raw(0), Uid::from_raw(0), Uid::from_raw(0))
        .with_context(|_| err_msg("setuid"))?;
    setresgid(Gid::from_raw(0), Gid::from_raw(0), Gid::from_raw(0))
        .with_context(|_| err_msg("setgid"))?;

    setgroups(&[Gid::from_raw(0)]).with_context(|_| err_msg("setgroups(0)"))?;

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

        ForkResult::Child => {
            let e = setup_pid_1(recv, send).void_unwrap_err();
            eprintln!("sandbox setup pid1 failed: {:?}", e);
            process::exit(67);
        }
    }
}

fn setup_pid_1(recv: os_pipe::PipeReader, send: os_pipe::PipeWriter) -> Result<void::Void, Error> {
    use nix::unistd::*;

    {
        let us = getpid().as_raw();
        ensure!(1 == us, "we failed to actually end up as pid 1: {}", us);
    }

    info!(
        "root: {:?}",
        fs::read_dir("/")?
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>()
    );

    assert!(fs::metadata("/bin")
        .with_context(|_| err_msg("confirming /bin is in place"))?
        .is_dir());
    assert!(fs::metadata("/bin/finit")
        .with_context(|_| err_msg("confirming our init is in place"))?
        .is_file());

    {
        let sticky_for_all = fs::Permissions::from_mode(0o1777);
        fs::set_permissions("/tmp", sticky_for_all.clone())
            .with_context(|_| err_msg("permissions for /tmp"))?;
        fs::set_permissions("/var/tmp", sticky_for_all)
            .with_context(|_| err_msg("permissions for /var/tmp"))?;
        // TODO: dev/shm?
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

        fs::remove_dir(".host-proc").with_context(|_| err_msg("dropping host-proc"))?;

        mount(
            Some("/"),
            "/",
            unset,
            MsFlags::MS_BIND | MsFlags::MS_NOSUID | MsFlags::MS_REMOUNT,
            unset,
        )
        .with_context(|_| err_msg("finalising /"))?;
    }

    let recv = dup(recv.as_raw_fd()).with_context(|_| err_msg("copying recv handle"))?;
    let send = dup(send.as_raw_fd()).with_context(|_| err_msg("copying send"))?;

    let proc = CString::new("/bin/finit")?;
    let argv0 = proc.clone();
    let recv = CString::new(format!("{}", recv))?;
    let send = CString::new(format!("{}", send))?;

    void::unreachable(
        execv(&proc, &[&argv0, &recv, &send]).with_context(|_| err_msg("exec finit"))?,
    );
}

fn make_mount_destination(name: &'static str) -> Result<(), Error> {
    let _ = fs::remove_dir(name);
    fs::create_dir(name)
        .with_context(|_| format_err!("creating {} before mounting on it", name))?;
    fs::set_permissions(name, fs::Permissions::from_mode(0o644))?;
    Ok(())
}
