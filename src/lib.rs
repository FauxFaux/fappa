pub mod build;
pub mod fetch_images;
pub mod git;
pub mod namespace;
pub mod specs;
pub mod unpack;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Release {
    DebianJessie,
    DebianStretch,
    DebianBuster,
    UbuntuTrusty,
    UbuntuXenial,
    UbuntuBionic,
    UbuntuCosmic,
    UbuntuDisco,
}

pub const RELEASES: [Release; 8] = [
    // best
    Release::UbuntuBionic,
    Release::DebianStretch,
    // older but supported
    Release::UbuntuXenial,
    Release::UbuntuTrusty,
    Release::DebianJessie,
    // pre-release
    Release::UbuntuCosmic,
    Release::UbuntuDisco,
    Release::DebianBuster,
];

impl Release {
    pub fn distro(&self) -> &'static str {
        use crate::Release::*;
        match self {
            DebianJessie | DebianStretch | DebianBuster => "debian",
            UbuntuTrusty | UbuntuXenial | UbuntuBionic | UbuntuCosmic | UbuntuDisco => "ubuntu",
        }
    }

    pub fn codename(&self) -> &'static str {
        use crate::Release::*;
        match self {
            DebianJessie => "jessie",
            DebianStretch => "stretch",
            DebianBuster => "buster",
            UbuntuTrusty => "trusty",
            UbuntuXenial => "xenial",
            UbuntuBionic => "bionic",
            UbuntuCosmic => "cosmic",
            UbuntuDisco => "disco",
        }
    }

    /// Older distros lack the locales-all package, which makes the locale
    /// environment a lot more sane for builds. Perhaps we should generate some
    /// extra locales on these distros?
    pub fn locales_all(&self) -> bool {
        use crate::Release::*;
        match self {
            DebianJessie => false,
            DebianStretch => true,
            DebianBuster => true,
            UbuntuTrusty => false,
            UbuntuXenial => true,
            UbuntuBionic => true,
            UbuntuCosmic => true,
            UbuntuDisco => true,
        }
    }
}
