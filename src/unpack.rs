use std::fs;
use std::io;
use std::path::Path;

use failure::Error;

pub fn unpack<S: AsRef<Path>, D: AsRef<Path>>(src: S, dest: D) -> Result<(), Error> {
    let file = fs::File::open(src)?;
    let unzipped = flate2::bufread::GzDecoder::new(io::BufReader::new(file));
    tar::Archive::new(unzipped).unpack(dest)?;
    Ok(())
}
