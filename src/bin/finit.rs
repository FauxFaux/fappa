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

use byteorder::ByteOrder;
use byteorder::WriteBytesExt;
use byteorder::LE;
use cast::u64;
use cast::usize;
use failure::bail;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use failure::ResultExt;
use nix::unistd;

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
        recv: unsafe { os_pipe::PipeReader::from_raw_fd(recv) },
        send: unsafe { os_pipe::PipeWriter::from_raw_fd(send) },
    };

    close_fds_except(&mut host, &[0, 1, 2, recv, send]).with_context(|_| err_msg("closing fds"))?;

    host.println("I'm alive, the init with the second face.")?;

    match work(&mut host) {
        Ok(()) => {
            host.write_msg(2, &[])?;
            Ok(())
        }
        Err(e) => {
            host.println(format!("failure: {:?}", e))?;
            host.write_msg(3, &[])?;
            Err(e)
        }
    }
}

fn work(host: &mut Host) -> Result<(), Error> {
    for p in psutil::process::all()? {
        host.println(format!("{} {:?}", p.pid, p.cmdline()?))?;
    }

    host.write_msg(4, &[])?;

    loop {
        let (code, data) = host.read_msg()?;
        match code {
            100 => run(host, data, false)?,
            101 => return Ok(()),
            102 => run(host, data, true)?,
            _ => bail!("unsupported code: {}", code),
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
        builder.before_exec(|| {
            drop_caps()?;
            unistd::setuid(unistd::Uid::from_raw(212))
                .map_err(nix_to_io)?;
            let gid = unistd::Gid::from_raw(212);
            unistd::setgid(gid).map_err(nix_to_io)?;
            unistd::setgroups(&[gid]).map_err(nix_to_io)?;

            Ok(())
        });
    }

    let mut proc = builder
        .spawn()
        .with_context(|_| err_msg("launching script runner"))?;

    let driven = drive_child(host, &mut proc, &data);

    let exit = proc
        .wait()
        .with_context(|_| err_msg("waiting for finished process"))?;

    host.println(format!("child: {:?}: {:?}", exit, driven))?;

    host.write_msg(6, &[exit.code().unwrap_or(255) as u8])?;

    Ok(())
}

fn drive_child(host: &mut Host, proc: &mut process::Child, data: &[u8]) -> Result<(), Error> {
    {
        proc.stdin
            .take()
            .ok_or_else(|| err_msg("stdin requested"))?
            .write_all(&data)
            .with_context(|_| err_msg("sending script to shell"))?;
    }

    let stdout = proc
        .stdout
        .as_mut()
        .ok_or_else(|| err_msg("stdout requested"))?;

    loop {
        let mut buf = [0u8; 1024 * 16];
        let valid = stdout.read(&mut buf)?;
        if 0 == valid {
            break;
        }

        host.write_msg(5, &buf[..valid])?;
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
    recv: os_pipe::PipeReader,
    send: os_pipe::PipeWriter,
}

impl Host {
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

    fn println<D: Display>(&mut self, msg: D) -> Result<(), Error> {
        self.write_msg(1, format!("{}", msg).as_bytes())?;
        match self.read_msg()? {
            (0, ref v) if v.is_empty() => Ok(()),
            (other, _) => bail!("unexpected print response: {}", other),
        }
    }
}

fn nix_to_io(e: nix::Error) -> io::Error {
    match e {
        nix::Error::Sys(e) => e.into(),
        e => io::Error::new(io::ErrorKind::InvalidData, format!("{:?}", e)),
    }
}
