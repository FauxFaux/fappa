use std::env;
use std::fmt::Display;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::RawFd;
use std::process;

use anyhow::bail;
use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Error;
use anyhow::Context;
use nix::unistd;

use fappa::namespace::child::{CodeFrom, CodeTo, Proto};

fn main() -> Result<(), Error> {
    assert_eq!(
        unistd::Pid::from_raw(1),
        unistd::getpid(),
        "we're expecting to be running as init (pid 1)!"
    );

    assert_eq!(3, env::args().len());
    let recv = env::args().nth(1).unwrap().parse()?;
    let send = env::args().nth(2).unwrap().parse()?;

    let mut host = Host {
        proto: Proto {
            recv: unsafe { os_pipe::PipeReader::from_raw_fd(recv) },
            send: unsafe { os_pipe::PipeWriter::from_raw_fd(send) },
            _types: Default::default(),
        },
    };

    close_fds_except(&mut host, &[0, 1, 2, recv, send]).with_context(|| anyhow!("closing fds"))?;

    host.println("I'm alive, the init with the second face.")?;

    match work(&mut host) {
        Ok(()) => {
            host.proto.write_msg(CodeFrom::ShutdownSuccess, &[])?;
            Ok(())
        }
        Err(e) => {
            host.proto.write_msg(
                CodeFrom::ShutdownError,
                format!("failure: {:?}", e).as_bytes(),
            )?;
            Err(e)
        }
    }
}

fn work(host: &mut Host) -> Result<(), Error> {
    for p in psutil::process::processes()? {
        let p = p?;
        host.println(format!("{} {:?}", p.pid(), p.cmdline()?))?;
    }

    host.proto.write_msg(CodeFrom::Ready, &[])?;

    loop {
        let (code, data) = host.proto.read_msg()?;
        match code {
            CodeTo::RunWithoutRoot => run(host, data, false)?,
            CodeTo::RunAsRoot => run(host, data, true)?,
            CodeTo::Die => return Ok(()),
            _ => bail!("unsupported code: {:?}", code),
        };
    }
}

fn run(host: &mut Host, data: Vec<u8>, root: bool) -> Result<(), Error> {
    use std::os::unix::process::CommandExt;

    let mut builder = process::Command::new("/bin/dash");

    builder
        .arg("-c")
        .arg("/bin/bash 2>&1")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::null());

    if !root {
        unsafe {
            builder.pre_exec(|| {
                drop_caps()?;
                unistd::setuid(unistd::Uid::from_raw(212)).map_err(nix_to_io)?;
                let gid = unistd::Gid::from_raw(212);
                unistd::setgid(gid).map_err(nix_to_io)?;
                unistd::setgroups(&[gid]).map_err(nix_to_io)?;

                Ok(())
            })
        };
    }

    let mut proc = builder
        .spawn()
        .with_context(|| anyhow!("launching script runner"))?;

    let driven = drive_child(host, &mut proc, &data);

    let exit = proc
        .wait()
        .with_context(|| anyhow!("waiting for finished process"))?;

    host.println(format!("child: {:?}: {:?}", exit, driven))?;

    host.proto
        .write_msg(CodeFrom::SubExited, &[exit.code().unwrap_or(255) as u8])?;

    Ok(())
}

fn drive_child(host: &mut Host, proc: &mut process::Child, data: &[u8]) -> Result<(), Error> {
    {
        proc.stdin
            .take()
            .ok_or_else(|| anyhow!("stdin requested"))?
            .write_all(&data)
            .with_context(|| anyhow!("sending script to shell"))?;
    }

    let stdout = proc
        .stdout
        .as_mut()
        .ok_or_else(|| anyhow!("stdout requested"))?;

    loop {
        let mut buf = [0u8; 1024 * 16];
        let valid = stdout.read(&mut buf)?;
        if 0 == valid {
            break;
        }

        host.proto.write_msg(CodeFrom::Output, &buf[..valid])?;
    }

    Ok(())
}

fn close_fds_except(host: &mut Host, leave: &[RawFd]) -> Result<(), Error> {
    let safety_block = 20;
    for i in 0..=safety_block {
        if leave.contains(&i) {
            continue;
        }
        let _ = nix::unistd::close(i);
    }

    for f in fs::read_dir("/proc/self/fd")? {
        let f = f?;
        let desc = f
            .file_name()
            .to_str()
            .ok_or_else(|| format_err!("bad fd content: {:?}", f.file_name()))?
            .parse()?;

        if desc < safety_block || leave.contains(&desc) {
            continue;
        }

        host.println(format!(
            "You leaked an fd! {:?} - {:?}",
            f.file_name(),
            fs::read_link(f.path())
        ))?;

        nix::unistd::close(desc)?;
    }

    nix::unistd::dup3(
        fs::File::open("/dev/null")?.as_raw_fd(),
        0,
        nix::fcntl::OFlag::empty(),
    )?;

    Ok(())
}

fn drop_caps() -> io::Result<()> {
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
    // 0b0010_1111 == that value, which isn't currently exposed by libc::.
    unsafe { libc::prctl(libc::PR_SET_SECUREBITS, 0b0010_1111, 0, 0, 0) };

    // TODO: should probably not allocate here (due to pre_exec).
    let max_cap: libc::c_int = fs::read_to_string("/proc/sys/kernel/cap_last_cap")?
        .trim()
        .parse()
        .ok()
        .filter(|&v| v > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "reading last_cap"))?;

    for cap in 0..=max_cap {
        if 0 != unsafe { libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0) } {
            use nix::errno::Errno;
            let err = Errno::last();
            match err {
                Errno::EINVAL => (),
                e => Err(e)?,
            }
        }
    }

    Ok(())
}

struct Host {
    proto: Proto<CodeFrom, CodeTo>,
}

impl Host {
    fn println<D: Display>(&mut self, msg: D) -> Result<(), Error> {
        self.proto
            .write_msg(CodeFrom::DebugOutput, format!("{}", msg).as_bytes())?;
        match self.proto.read_msg()? {
            (CodeTo::Ack, ref v) if v.is_empty() => Ok(()),
            (other, _) => bail!("unexpected print response: {:?}", other),
        }
    }
}

fn nix_to_io(e: nix::Error) -> io::Error {
    match e {
        nix::Error::Sys(e) => e.into(),
        e => io::Error::new(io::ErrorKind::InvalidData, format!("{:?}", e)),
    }
}
