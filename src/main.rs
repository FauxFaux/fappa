#[macro_use]
extern crate failure;

extern crate handlebars;

#[macro_use]
extern crate serde_json;
extern crate shiplift;
extern crate tempdir;

use std::fs;
use std::io::Write;

use failure::Error;
use shiplift::BuildOptions;
use shiplift::Docker;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Release {
    DebianJessie,
    DebianStretch,
    DebianBuster,
    UbuntuTrusty,
    UbuntuXenial,
    UbuntuArtful,
    UbuntuBionic,
    UbuntuCosmic,
}

impl Release {
    fn distro(&self) -> &'static str {
        use Release::*;
        match self {
            | DebianJessie | DebianStretch | DebianBuster => "debian",
            | UbuntuTrusty | UbuntuXenial | UbuntuArtful | UbuntuBionic | UbuntuCosmic => "ubuntu",
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
            | UbuntuCosmic => "cosmic",
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
            | UbuntuCosmic => true,
        }
    }
}

fn build_template(docker: &Docker, release: Release) -> Result<(), Error> {
    let dir = tempdir::TempDir::new("fappa")?;
    let from = format!("{}:{}", release.distro(), release.codename());

    {
        let mut dockerfile = dir.path().to_path_buf();
        dockerfile.push("Dockerfile");
        let mut dockerfile = fs::File::create(dockerfile)?;

        let reg = handlebars::Handlebars::new();
        reg.render_template_to_write(
            include_str!("build.Dockerfile.hbs"),
            &json!({
            "from": from,
            "locales": if release.locales_all() { "locales-all" } else { "locales" },
        }),
            &mut dockerfile,
        )?;

        for (file, content) in &[
            (
                "drop-privs-harder.c",
                &include_bytes!("../security-tools/drop-privs-harder.c")[..],
            ),
            (
                "drop-all-caps.c",
                &include_bytes!("../security-tools/drop-all-caps.c")[..],
            ),
            (
                "all-caps.h",
                &include_bytes!("../security-tools/all-caps.h")[..],
            ),
        ] {
            let mut new_file = dir.path().to_path_buf();
            new_file.push(file);
            fs::File::create(new_file)?.write_all(content)?;
        }
    }

    let dir_as_str = dir
        .path()
        .as_os_str()
        .to_str()
        .ok_or(format_err!("unrepresentable path and dumb library"))?;

    for line in docker.images().build(&BuildOptions::builder(dir_as_str)
        .tag(format!("fappa-{}", release.codename()))
        .network_mode("mope")
        .build())?
    {
        let line = line
            .as_object()
            .ok_or_else(|| format_err!("unexpected line: {:?}", line))?;
        if let Some(msg) = line.get("stream").and_then(|stream| stream.as_string()) {
            for line in msg.trim_right_matches('\n').split('\n') {
                println!(
                    "[{}] log: {}",
                    release.codename(),
                    line.replace(|c| u32::from(c) < 32, " ")
                );
            }
        } else if let Some(aux) = line.get("aux").and_then(|aux| aux.as_object()) {
            println!("[{}] aux: {:?}", release.codename(), aux)
        } else {
            bail!("unknown notification: {:?}", line);
        }
    }

    Ok(())
}

fn build_templates() -> Result<(), Error> {
    let docker = shiplift::Docker::new();

    for release in &[
        // best
        Release::UbuntuBionic,
        Release::DebianStretch,
        // older but supported
        Release::UbuntuXenial,
        Release::UbuntuTrusty,
        Release::UbuntuArtful,
        Release::DebianJessie,
        // pre-release
        Release::UbuntuCosmic,
        Release::DebianBuster,
    ] {
        build_template(&docker, *release)?;
    }

    Ok(())
}

fn main() -> Result<(), Error> {
    build_templates()?;
    Ok(())
}
