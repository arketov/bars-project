use super::{WorkerHandler, errors::worker_error};
use bars_protocol::{ErrorKind, INTERNAL_ERROR, RpcError, TextSearchOutput, TextSearchParams};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) type ActiveSearches = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

impl WorkerHandler {
    pub(super) async fn text_search(
        &self,
        params: TextSearchParams,
    ) -> Result<TextSearchOutput, RpcError> {
        if !Path::new(&params.project).is_absolute() {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "Нужен абсолютный путь проекта",
            ));
        }
        let project = self.worker.resolve(&params.project).map_err(worker_error)?;
        let docs = self.worker.docs_root(&project).map_err(worker_error)?;
        let stop = Arc::new(AtomicBool::new(false));
        let key = params.window_id.clone();
        let active = Arc::clone(&self.text_searches);
        if let Some(previous) = active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.clone(), Arc::clone(&stop))
        {
            previous.store(true, Ordering::Relaxed);
        }
        // Уборка внутри блокирующей задачи: отмена ожидающего async-вызова не оставляет запись навсегда.
        tokio::task::spawn_blocking(move || {
            let result = crate::text_search::search(&project, Path::new(&docs), &params, &stop);
            let mut active = active.lock().unwrap_or_else(|e| e.into_inner());
            if active
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, &stop))
            {
                active.remove(&key);
            }
            result
        })
        .await
        .map_err(|e| RpcError::standard(INTERNAL_ERROR, e.to_string()))?
    }
}
