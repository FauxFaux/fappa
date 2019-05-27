use std::io::Read;
use std::io::Write;

use byteorder::ByteOrder;
use byteorder::WriteBytesExt;
use byteorder::LE;
use cast::u64;
use cast::usize;
use enum_primitive_derive::Primitive;
use failure::bail;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use failure::ResultExt;
use num_traits::FromPrimitive;
use num_traits::ToPrimitive;

#[derive(Primitive, Copy, Clone, PartialEq, Eq)]
pub enum CodeFrom {
    DebugOutput = 1,
    ShutdownSuccess = 2,
    ShutdownError = 3,
    Ready = 4,
    Output = 5,
    SubExited = 6,
}

#[derive(Primitive, Copy, Clone, PartialEq, Eq)]
pub enum CodeTo {
    Ack = 100,
    RunAsRoot = 101,
    RunWithoutRoot = 102,
    Die = 103,
}

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
        match CodeFrom::from_u64(code) {
            Some(CodeFrom::DebugOutput) => {
                self.write_msg(CodeTo::Ack, &[])?;
                Ok(Some(FromChild::Debug(String::from_utf8(data)?)))
            }
            Some(CodeFrom::ShutdownSuccess) => Ok(None),
            Some(CodeFrom::ShutdownError) => Err(err_msg(String::from_utf8(data)?)),
            Some(CodeFrom::Ready) => Ok(Some(FromChild::Ready)),
            Some(CodeFrom::Output) => Ok(Some(FromChild::Output(data))),
            Some(CodeFrom::SubExited) => Ok(Some(FromChild::SubExited(data[0]))),
            // TODO: should we tell the client to die here?
            None => bail!("unsupported client code: {}", code),
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

    pub fn write_msg(&mut self, code: CodeTo, data: &[u8]) -> Result<(), Error> {
        let total = 16 + data.len();
        let mut msg = Vec::with_capacity(total);
        // header: length (including header), code
        msg.write_u64::<LE>(u64(total))?;
        msg.write_u64::<LE>(code.to_u64().expect("static derivation"))?;

        // data:
        msg.extend_from_slice(data);
        self.send.write_all(&msg)?;
        Ok(())
    }
}
