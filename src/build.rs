use std::fs;
use std::io::Write;

use failure::Error;
use tempdir;

use specs::Command;
use specs::Package;
use Release;

pub fn build(release: &Release, package: &Package) -> Result<(), Error> {
    let dir = tempdir::TempDir::new("fappa")?;
    {
        let mut dockerfile = dir.path().to_path_buf();
        dockerfile.push("Dockerfile");
        let mut dockerfile = fs::File::create(dockerfile)?;

        writeln!(dockerfile, "FROM fappa-{}", release.codename())?;
        writeln!(dockerfile, "WORKDIR /build")?;

        for command in &package.source {
            match command {
                Command::Clone { repo, dest } => {
                    writeln!(dockerfile, "RUN git clone /repo/{} {}", repo, dest)?
                }
                _ => unimplemented!("source: {:?}", command),
            }
        }
    }

    unimplemented!();
}
