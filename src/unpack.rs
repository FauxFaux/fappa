use std::fs;
use std::io;
use std::path::Path;

use anyhow::ensure;
use anyhow::Error;

pub fn unpack<S: AsRef<Path>, D: AsRef<Path>>(src: S, dest: D) -> Result<(), Error> {
    let dest = dest.as_ref();
    let file = fs::File::open(src)?;
    let unzipped = zstd::Decoder::new(file)?;
    tar::Archive::new(unzipped).unpack(dest)?;
    let mut bin = dest.to_path_buf();
    bin.push("bin");
    ensure!(bin.metadata()?.is_dir(), "unpacked image contains a /bin");
    Ok(())
}
