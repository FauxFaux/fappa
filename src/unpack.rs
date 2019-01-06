use std::fs;
use std::io;

use failure::Error;

pub fn unpack(src: &str, dest: &str) -> Result<(), Error> {
    let file = fs::File::open(src)?;
    let unzipped = flate2::bufread::GzDecoder::new(io::BufReader::new(file));
    tar::Archive::new(unzipped).unpack(dest)?;
    Ok(())
}
