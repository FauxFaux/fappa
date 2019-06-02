use std::convert::TryInto;
use std::io::Read;
use std::io::Write;
use std::marker::PhantomData;

use cast::u64;
use cast::usize;
use enum_primitive_derive::Primitive;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use failure::ResultExt;

#[derive(Primitive, Copy, Clone, Debug, PartialEq, Eq)]
pub enum CodeFrom {
    DebugOutput = 1,
    ShutdownSuccess = 2,
    ShutdownError = 3,
    Ready = 4,
    Output = 5,
    SubExited = 6,
}

#[derive(Primitive, Copy, Clone, Debug, PartialEq, Eq)]
pub enum CodeTo {
    Ack = 100,
    RunAsRoot = 101,
    RunWithoutRoot = 102,
    Die = 103,
}

pub struct Proto<S, R> {
    pub send: os_pipe::PipeWriter,
    pub recv: os_pipe::PipeReader,
    pub _types: (PhantomData<S>, PhantomData<R>),
}

pub struct Child {
    pub proto: Proto<CodeTo, CodeFrom>,
    pub pid: nix::unistd::Pid,
}

#[derive(Debug, Clone)]
pub enum FromChild {
    Debug(String),
    Ready,
    Output(Vec<u8>),
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
        let (code, data) = self.proto.read_msg()?;
        match code {
            CodeFrom::DebugOutput => {
                self.proto.write_msg(CodeTo::Ack, &[])?;
                Ok(Some(FromChild::Debug(String::from_utf8(data)?)))
            }
            CodeFrom::ShutdownSuccess => Ok(None),
            CodeFrom::ShutdownError => Err(err_msg(String::from_utf8(data)?)),
            CodeFrom::Ready => Ok(Some(FromChild::Ready)),
            CodeFrom::Output => Ok(Some(FromChild::Output(data))),
            CodeFrom::SubExited => Ok(Some(FromChild::SubExited(data[0]))),
        }
    }
}

impl<S: num_traits::ToPrimitive, R: num_traits::FromPrimitive> Proto<S, R> {
    pub fn read_msg(&mut self) -> Result<(R, Vec<u8>), Error> {
        let mut buf = [0u8; 16];
        self.recv
            .read_exact(&mut buf)
            .with_context(|_| err_msg("reading header from child"))?;
        let len = u64::from_le_bytes(buf[..=8].try_into().expect("fixed slice"));
        let code = u64::from_le_bytes(buf[8..].try_into().expect("fixed slice"));
        let code = R::from_u64(code).ok_or_else(|| format_err!("invalid command: {}", code))?;
        let mut buf = vec![0u8; usize(len - 16)];
        self.recv
            .read_exact(&mut buf)
            .with_context(|_| format_err!("reading {}-16 bytes from child", len))?;
        Ok((code, buf))
    }

    pub fn write_msg(&mut self, code: S, data: &[u8]) -> Result<(), Error> {
        let total = 16 + data.len();
        let mut msg = Vec::with_capacity(total);
        // header: length (including header), code
        msg.extend_from_slice(&u64(total).to_le_bytes());
        msg.extend_from_slice(&code.to_u64().expect("static derivation").to_le_bytes());

        // data:
        msg.extend_from_slice(data);
        self.send.write_all(&msg)?;
        Ok(())
    }
}
