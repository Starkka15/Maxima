pub mod dip;
pub mod pre_dip;

use dip::DiPManifest;
use pre_dip::PreDiPManifest;
use quick_xml::DeError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Xml(#[from] DeError),
    #[error(transparent)]
    Native(#[from] crate::util::native::NativeError),
    #[error(transparent)]
    Registry(#[from] crate::util::registry::RegistryError),

    #[error("failed to decode DiPManifest. Weird encoding?")]
    Decode,
    #[error("Unsupported Manifest.\nDiP Attempt: `{dip_attempt:?}`\nPreDiP Attempt: `{pre_dip_attempt:?}`")]
    Unsupported {
        dip_attempt: Box<ManifestError>,
        pre_dip_attempt: Box<ManifestError>,
    },
    #[error("could not find install path for `{0}`")]
    NoInstallPath(String),
}

pub const MANIFEST_RELATIVE_PATH: &str = "__Installer/installerdata.xml";

#[async_trait::async_trait]
pub trait GameManifest: Send + std::fmt::Debug {
    async fn run_touchup(&self, install_path: &PathBuf) -> Result<(), ManifestError>;
    fn execute_path(&self, trial: bool) -> Option<String>;
    fn version(&self) -> Option<String>;
}
#[async_trait::async_trait]
impl GameManifest for DiPManifest {
    async fn run_touchup(&self, install_path: &PathBuf) -> Result<(), ManifestError> {
        self.run_touchup(install_path).await
    }

    fn execute_path(&self, trial: bool) -> Option<String> {
        self.execute_path(trial)
    }

    fn version(&self) -> Option<String> {
        self.version()
    }
}

#[async_trait::async_trait]
impl GameManifest for PreDiPManifest {
    async fn run_touchup(&self, install_path: &PathBuf) -> Result<(), ManifestError> {
        self.run_touchup(install_path).await
    }

    fn execute_path(&self, _: bool) -> Option<String> {
        None // pre-dip games don't have an exe field, most if not all just use info in the offer
    }

    fn version(&self) -> Option<String> {
        self.version()
    }
}

pub async fn read(path: PathBuf) -> Result<Box<dyn GameManifest>, ManifestError> {
    let dip_attempt = DiPManifest::read(&path).await;
    if let Ok(manifest) = dip_attempt {
        return Ok(Box::new(manifest));
    }
    let pre_dip_attempt = PreDiPManifest::read(&path).await;
    if let Ok(manifest) = pre_dip_attempt {
        return Ok(Box::new(manifest));
    }

    Err(ManifestError::Unsupported {
        dip_attempt: dip_attempt.unwrap_err().into(),
        pre_dip_attempt: pre_dip_attempt.unwrap_err().into(),
    })
}
