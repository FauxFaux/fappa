use std::io::Read;
use std::io::Write;

use byteorder::ByteOrder;
use byteorder::WriteBytesExt;
use byteorder::LE;
use cast::u64;
use cast::usize;
use failure::err_msg;
use failure::Error;
use failure::ResultExt;

#[derive(Debug)]
pub struct Child {
    pub send: os_pipe::PipeWriter,
    pub recv: os_pipe::PipeReader,
    pub pid: nix::unistd::Pid,
}

#[derive(Debug, Clone)]
pub enum FromChild {
    // 1
    Debug(String),
    // 4
    Ready,
    // 5
    Output(Vec<u8>),
    // 6
    SubExited(u8),
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
        match code {
            1 => {
                self.write_msg(0, &[])?;
                Ok(Some(FromChild::Debug(String::from_utf8(data)?)))
            }
            2 => Ok(None),
            3 => Err(err_msg(String::from_utf8(data)?)),
            4 => Ok(Some(FromChild::Ready)),
            5 => Ok(Some(FromChild::Output(data))),
            6 => Ok(Some(FromChild::SubExited(data[0]))),
            // TODO: should we tell the client to die here?
            code => bail!("unsupported client code: {}", code),
        }
    }

    fn read_msg(&mut self) -> Result<(u64, Vec<u8>), Error> {
        let mut buf = [0u8; 16];
        self.recv
            .read_exact(&mut buf)
            .with_context(|_| err_msg("reading header from child"))?;
        let len = LE::read_u64(&buf[..=8]);
        let code = LE::read_u64(&buf[8..]);
        let mut buf = vec![0u8; usize(len - 16)];
        self.recv
            .read_exact(&mut buf)
            .with_context(|_| format_err!("reading {}-16 bytes from child", len))?;
        Ok((code, buf))
    }

    pub fn write_msg(&mut self, code: u64, data: &[u8]) -> Result<(), Error> {
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
