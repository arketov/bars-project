//! Локальный фальшивый HTTP-сервер для тестов внешних моделей: настоящие провайдеры и их ключи
//! в тестах не используются.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Принятый запрос.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// Заголовки с именами в нижнем регистре.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("тело запроса — JSON")
    }
}

/// Ответ обработчика.
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn ok(body: serde_json::Value) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
        }
    }
}

type Handler = dyn Fn(&Request) -> Response + Send + Sync;

/// HTTP-сервер на свободном порту `127.0.0.1`; останавливается при `drop`.
pub struct FakeServer {
    addr: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FakeServer {
    pub fn start(handler: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("свободный порт");
        let addr = listener.local_addr().expect("адрес сервера");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let thread = {
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let requests = Arc::clone(&requests);
                    let handler = Arc::clone(&handler);
                    std::thread::spawn(move || serve(stream, &requests, handler.as_ref()));
                }
            })
        };
        Self {
            addr,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    /// Адрес для `base_url`: `http://127.0.0.1:<порт>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Все принятые запросы по порядку.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Будит `accept`, чтобы поток увидел флаг.
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(stream: TcpStream, requests: &Mutex<Vec<Request>>, handler: &Handler) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut writer = stream;
    {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();

        let mut headers = Vec::new();
        let mut length = 0usize;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                return;
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                let name = name.trim().to_lowercase();
                let value = value.trim().to_string();
                if name == "content-length" {
                    length = value.parse().unwrap_or(0);
                }
                headers.push((name, value));
            }
        }
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let request = Request {
            method,
            path,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        };
        requests.lock().unwrap().push(request.clone());
        let response = handler(&request);
        let reply = format!(
            "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.status,
            response.body.len(),
            response.body
        );
        let _ = writer.write_all(reply.as_bytes());
        let _ = writer.flush();
    }
}
