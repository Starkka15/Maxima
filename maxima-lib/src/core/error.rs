use thiserror::Error;

#[derive(Error, Debug)]
pub enum BackgroundServiceClientError {
    #[error(transparent)]
    Native(#[from] crate::util::native::NativeError),
    #[cfg(windows)]
    #[error(transparent)]
    Inject(#[from] dll_syringe::error::InjectError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Registry(#[from] crate::util::registry::RegistryError),

    #[error("request failed: `{0}`")]
    Request(String),
    #[error("attempted to inject into invalid process")]
    InvalidInjectionTarget,
}

#[derive(Error, Debug)]
pub enum CacheRetrievalError {
    #[error(transparent)]
    Native(#[from] crate::util::native::NativeError),
    #[error(transparent)]
    ServiceLayer(#[from] crate::core::service_layer::ServiceLayerError),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("incapable of pulling {0} from cache")]
    Incapable(String),
}
