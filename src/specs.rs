use std::fs;
use std::path::Path;

use failure::Error;
use failure::ResultExt;
use toml;
use walkdir;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct Spec {
    package: Vec<Package>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Package {
    name: String,
    source: String,
}

pub fn load_from<P: AsRef<Path>>(dir: P) -> Result<Vec<Package>, Error> {
    let mut ret = Vec::new();

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;

        if entry.file_type().is_dir() {
            continue;
        }

        if entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
        {
            continue;
        }

        let spec: Spec = toml::from_slice(
            &fs::read(entry.path()).with_context(|_| format!("opening {:?}", entry.path()))?
        )?;
        ret.extend(spec.package);
    }

    Ok(ret)
}
