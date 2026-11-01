//! Только завершённый обмен возвращает канал в пул: отменённый future закрывает его.
use std::sync::{Arc, Mutex};

use bars_protocol::Client;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) const CAPACITY: usize = 4;

pub(crate) struct Pool {
    idle: Mutex<Vec<Client>>,
    permits: Arc<Semaphore>,
    // Холодный старт нескольких вызовов запускает воркер только один раз.
    pub connect: tokio::sync::Mutex<()>,
}

impl Default for Pool {
    fn default() -> Self {
        Self {
            idle: Mutex::new(Vec::with_capacity(CAPACITY)),
            permits: Arc::new(Semaphore::new(CAPACITY)),
            connect: tokio::sync::Mutex::new(()),
        }
    }
}

impl Pool {
    pub async fn acquire(&self) -> Lease<'_> {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .expect("пул открыт");
        let client = self.idle.lock().expect("пул соединений").pop();
        Lease {
            pool: self,
            client,
            _permit: permit,
        }
    }
}

pub(crate) struct Lease<'a> {
    pool: &'a Pool,
    pub client: Option<Client>,
    _permit: OwnedSemaphorePermit,
}

impl Lease<'_> {
    pub fn recycle(mut self) {
        if let Some(client) = self.client.take() {
            self.pool.idle.lock().expect("пул соединений").push(client);
        }
    }
}
