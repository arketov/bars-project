//! Последний владелец слота только передаёт сессию; дорогой drop — вне async-потоков.
use super::{Loads, lock};
use std::sync::{Arc, mpsc};

enum Command {
    Release(Box<dyn Send>),
    Flush(mpsc::Sender<()>),
}

pub(super) struct Retirement {
    sender: mpsc::Sender<Command>,
}

impl Retirement {
    pub fn new(loads: Arc<Loads>) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("bars-model-retirement".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Release(model) => {
                            let _gate = lock(&loads.gate);
                            // Ошибка destructor одной библиотеки не закрывает очередь.
                            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                drop(model)
                            }))
                            .is_err()
                            {
                                log::error!("Ошибка освобождения сессии модели");
                            }
                        }
                        Command::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .expect("не удалось запустить поток освобождения моделей");
        Self { sender }
    }

    pub fn release(&self, model: impl Send + 'static) {
        self.sender
            .send(Command::Release(Box::new(model)))
            .unwrap_or_else(|_| panic!("поток освобождения моделей остановлен"));
    }

    /// Только блокирующий поток shutdown; ждёт все уже переданные сессии.
    pub fn flush(&self) {
        let (done, wait) = mpsc::channel();
        self.sender
            .send(Command::Flush(done))
            .unwrap_or_else(|_| panic!("поток освобождения моделей остановлен"));
        wait.recv().expect("поток освобождения моделей остановлен");
    }
}
