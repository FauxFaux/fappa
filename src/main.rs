#[macro_use]
extern crate error_chain;
extern crate tempdir;

mod errors;

use std::fs;
use std::io::Write;
use std::process;

use errors::*;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Release {
    DebianJessie,
    DebianStretch,
    DebianBuster,
    UbuntuTrusty,
    UbuntuXenial,
    UbuntuArtful,
    UbuntuBionic,
}

impl Release {
    fn distro(&self) -> &'static str {
        use Release::*;
        match self {
            | DebianJessie | DebianStretch | DebianBuster => "debian",
            | UbuntuTrusty | UbuntuXenial | UbuntuArtful | UbuntuBionic => "ubuntu",
        }
    }

    fn codename(&self) -> &'static str {
        use Release::*;
        match self {
            | DebianJessie => "jessie",
            | DebianStretch => "stretch",
            | DebianBuster => "buster",
            | UbuntuTrusty => "trusty",
            | UbuntuXenial => "xenial",
            | UbuntuArtful => "artful",
            | UbuntuBionic => "bionic",
        }
    }

    /// Older distros lack the locales-all package, which makes the locale
    /// environment a lot more sane for builds. Perhaps we should generate some
    /// extra locales on these distros?
    fn locales_all(&self) -> bool {
        use Release::*;
        match self {
            | DebianJessie => false,
            | DebianStretch => true,
            | DebianBuster => true,
            | UbuntuTrusty => false,
            | UbuntuXenial => true,
            | UbuntuArtful => true,
            | UbuntuBionic => true,
        }
    }
}

fn build_template(release: Release) -> Result<()> {
    let dir = tempdir::TempDir::new("fappa")?;
    {
        let mut dockerfile = dir.path().to_path_buf();
        dockerfile.push("Dockerfile");
        let mut dockerfile = fs::File::create(dockerfile)?;

        let from = format!("{}:{}", release.distro(), release.codename());

        assert!(
            process::Command::new("docker")
                .args(&["pull", &from])
                .spawn()?
                .wait()?
                .success()
        );

        writeln!(dockerfile, "FROM {}", from)?;

        writeln!(
            dockerfile,
            "{}",
            r#"
RUN \
    echo 'Acquire::http { Proxy "http://urika:3142"; };' > /etc/apt/apt.conf.d/69docker && \
    apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get upgrade -y && \
    apt-get clean

RUN \
    DEBIAN_FRONTEND=noninteractive apt-get install -y \
        apt-utils \
        procps \"#
        )?;

        write!(
            dockerfile,
            "{}",
            if release.locales_all() {
                r"locales-all \"
            } else {
                r"locales \"
            }
        )?;

        writeln!(
            dockerfile,
            "{}",
            r#"
        && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y \
        bzr \
        git \
        mercurial \
        subversion \
        openssh-client \
        ca-certificates \
        curl \
        wget \
        gnupg2 \
        dirmngr && \
    apt-get clean

RUN \
    DEBIAN_FRONTEND=noninteractive apt-get install -y \
        autoconf \
		automake \
		bzip2 \
		file \
		g++ \
		gcc \
		imagemagick \
		libbz2-dev \
		libc6-dev \
		libcurl4-openssl-dev \
		libdb-dev \
		libevent-dev \
		libffi-dev \
		libgdbm-dev \
		libgeoip-dev \
		libglib2.0-dev \
		libjpeg-dev \
		libkrb5-dev \
		liblzma-dev \
		libmagickcore-dev \
		libmagickwand-dev \
		libncurses-dev \
		libpng-dev \
		libpq-dev \
		libreadline-dev \
		libsqlite3-dev \
		libssl-dev \
		libtool \
		libwebp-dev \
		libxml2-dev \
		libxslt-dev \
		libyaml-dev \
		make \
		patch \
		xz-utils \
		zlib1g-dev && \
    apt-get clean"#
        )?;
    }

    assert!(
        process::Command::new("docker")
            .arg("build")
            .arg(format!("--tag=fappa-{}", release.codename()))
            .arg("--network=mope")
            .arg(".")
            .current_dir(&dir)
            .spawn()?
            .wait()?
            .success()
    );

    Ok(())
}

fn build_templates() -> Result<()> {
    build_template(Release::UbuntuTrusty)?;
    build_template(Release::UbuntuXenial)?;
    build_template(Release::UbuntuArtful)?;
    build_template(Release::UbuntuBionic)?;
    build_template(Release::DebianJessie)?;
    build_template(Release::DebianStretch)?;
    build_template(Release::DebianBuster)?;
    Ok(())
}

fn run() -> Result<()> {
    build_templates()?;
    Ok(())
}

quick_main!(run);
