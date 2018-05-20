extern crate handlebars;

#[macro_use]
extern crate failure;

#[macro_use]
extern crate serde_json;
extern crate tempdir;

use std::fs;
use std::process;

use failure::Error;

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

fn build_template(release: Release) -> Result<(), Error> {
    let dir = tempdir::TempDir::new("fappa")?;
    let from = format!("{}:{}", release.distro(), release.codename());

    assert!(
        process::Command::new("docker")
            .args(&["pull", &from])
            .spawn()?
            .wait()?
            .success()
    );

    {
        let mut dockerfile = dir.path().to_path_buf();
        dockerfile.push("Dockerfile");
        let mut dockerfile = fs::File::create(dockerfile)?;


        let reg = handlebars::Handlebars::new();
        reg.render_template_to_write(include_str!("build.Dockerfile.hbs"), &json!({
            "from": from,
            "locales": if release.locales_all() { "locales-all" } else { "locales" },
        }), &mut dockerfile)?;
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

fn build_templates() -> Result<(), Error> {
    build_template(Release::UbuntuTrusty)?;
    build_template(Release::UbuntuXenial)?;
    build_template(Release::UbuntuArtful)?;
    build_template(Release::UbuntuBionic)?;
    build_template(Release::DebianJessie)?;
    build_template(Release::DebianStretch)?;
    build_template(Release::DebianBuster)?;
    Ok(())
}

fn main() -> Result<(), Error> {
    build_templates()?;
    Ok(())
}
