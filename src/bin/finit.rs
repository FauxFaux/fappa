use std::env;
use std::fmt::Display;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::os::unix::io::FromRawFd;

use byteorder::ByteOrder;
use byteorder::WriteBytesExt;
use byteorder::LE;
use cast::u64;
use cast::usize;
use failure::bail;
use failure::Error;
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

    host.write_msg(2, &[])?;
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
