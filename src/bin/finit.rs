use std::env;
use std::fmt::Display;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::os::unix::io::FromRawFd;
use std::process;

use byteorder::ByteOrder;
use byteorder::WriteBytesExt;
use byteorder::LE;
use cast::u64;
use cast::usize;
use failure::bail;
use failure::err_msg;
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

    for f in fs::read_dir("/proc/self/fd")? {
        let f = f?;
        host.println(format!(
            "{:?} - {:?}",
            f.file_name(),
            fs::read_link(f.path())
        ))?;
    }

    host.write_msg(4, &[])?;

    loop {
        let (code, data) = host.read_msg()?;
        match code {
            100 => run(host, data)?,
            101 => return Ok(()),
            _ => bail!("unsupported code: {}", code),
        };
    }
}

fn run(host: &mut Host, data: Vec<u8>) -> Result<(), Error> {
    use std::os::unix::process::CommandExt;

    let mut proc = process::Command::new("/bin/dash")
        .arg("-c")
        .arg("/bin/bash 2>&1")
        //        .uid(1000)
        //        .gid(1000)
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::null())
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
