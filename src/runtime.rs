use tokio::runtime::{Builder, Handle, Runtime};

pub struct AppRuntime {
    runtime: Runtime,
}

impl AppRuntime {
    pub fn new() -> Result<Self, String> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|err| err.to_string())?;
        Ok(Self { runtime })
    }

    pub fn handle(&self) -> Handle {
        self.runtime.handle().clone()
    }
}
