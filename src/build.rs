use std::fs;
use std::io::Write;

use failure::Error;
use fs_extra::dir;
use shiplift::rep::ContainerCreateInfo;
use shiplift::BuildOptions;
use shiplift::ContainerOptions;
use shiplift::Docker;
use tempdir;

use specs::Command;
use specs::Package;
use Release;

pub fn build(docker: &Docker, release: &Release, package: &Package) -> Result<(), Error> {
    let dir = tempdir::TempDir::new("fappa")?;
    {
        let mut dockerfile = dir.path().to_path_buf();
        dockerfile.push("Dockerfile");
        let mut dockerfile = fs::File::create(dockerfile)?;

        writeln!(dockerfile, "FROM fappa-{}", release.codename())?;
        writeln!(dockerfile, "WORKDIR /build")?;

        if !package.build_dep.is_empty() {
            writeln!(
                dockerfile,
                "RUN DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                package.build_dep.join(" ")
            )?;
        }

        for command in &package.source {
            match command {
                Command::Clone { repo, dest } => {
                    let ::git::LocalRepo { specifier, path } = ::git::check_cloned(repo)?;

                    dir::copy(format!(".cache/{}", path), &dir, &dir::CopyOptions::new())?;
                    writeln!(dockerfile, "COPY {} /repo/{}", path, path)?;
                    writeln!(
                        dockerfile,
                        "RUN git clone /repo/{} {} && (cd {} && git {})",
                        path,
                        dest,
                        dest,
                        specifier.git_args()
                    )?
                }
                _ => unimplemented!("source: {:?}", command),
            }
        }

        for command in &package.build {
            match command {
                Command::WorkDir(dir) => writeln!(dockerfile, "WORKDIR {}", dir)?,
                Command::Autoreconf => writeln!(
                    dockerfile,
                    "RUN autoreconf -fvi && ./configure --prefix=/usr/local && make -j 2"
                )?,
                _ => unimplemented!("build: {:?}", command),
            }
        }

        for command in &package.install {
            match command {
                Command::Run(what) => writeln!(dockerfile, "CMD {}", what)?,
                _ => unimplemented!("install: {:?}", command),
            }
        }
    }

    let built_id = ::dump_lines(
        *release,
        docker
            .images()
            .build(&BuildOptions::builder(::tempdir_as_bad_str(&dir)?)
                .network_mode("mope")
                .build())?,
    )?.ok_or_else(|| format_err!("build didn't build an id"))?;

    let containers = docker.containers();

    let ContainerCreateInfo { Id: id, .. } =
        containers.create(&ContainerOptions::builder(&built_id).build())?;

    println!("starting install container {}", id);
    let created = containers.get(&id);
    created.start()?;
    created.wait()?;
    println!("done!");
    Ok(())
}
