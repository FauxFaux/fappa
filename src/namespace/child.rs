use std::io::Read;
use std::io::Write;

use byteorder::ByteOrder;
use byteorder::WriteBytesExt;
use byteorder::LE;
use cast::u64;
use cast::usize;
use failure::Error;

#[derive(Debug)]
pub struct Child {
    pub send: os_pipe::PipeWriter,
    pub recv: os_pipe::PipeReader,
    pub pid: nix::unistd::Pid,
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
