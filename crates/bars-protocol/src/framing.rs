//! Строки канала: чтение с ограничением длины и запись (`docs/architecture/ipc.md`).

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Самая длинная строка запроса, байт. Запрос — параметры метода, мегабайта хватает с запасом.
pub const MAX_REQUEST_LINE: usize = 1 << 20;
/// Самая длинная строка ответа, байт: `read` возвращает документ целиком.
pub const MAX_RESPONSE_LINE: usize = 64 << 20;

/// Читает одну строку без перевода строки. `None` — собеседник закрыл соединение между строками.
///
/// Строка длиннее `max` байт — ошибка `InvalidData`: без ограничения локальный клиент мог бы заставить
/// воркер копить память. Соединение после такой ошибки закрывают. Обрыв посреди строки — `UnexpectedEof`.
pub async fn read_line_limited<R>(reader: &mut R, max: usize) -> io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut line: Vec<u8> = Vec::new();
    loop {
        let (found, used) = {
            let chunk = reader.fill_buf().await?;
            if chunk.is_empty() {
                return if line.is_empty() {
                    Ok(None)
                } else {
                    Err(io::ErrorKind::UnexpectedEof.into())
                };
            }
            match chunk.iter().position(|&b| b == b'\n') {
                Some(end) => {
                    line.extend_from_slice(&chunk[..end]);
                    (true, end + 1)
                }
                None => {
                    line.extend_from_slice(chunk);
                    (false, chunk.len())
                }
            }
        };
        reader.consume(used);
        if line.len() > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("строка длиннее {max} байт"),
            ));
        }
        if found {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return String::from_utf8(line)
                .map(Some)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "строка не в UTF-8"));
        }
    }
}

/// Пишет строку. Перевод строки добавляется, если его нет; `encode_line` его уже ставит.
pub async fn write_line<W>(writer: &mut W, line: &str) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(line.as_bytes()).await?;
    if !line.ends_with('\n') {
        writer.write_all(b"\n").await?;
    }
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader, duplex};

    async fn read_all(input: &[u8], max: usize) -> Vec<io::Result<Option<String>>> {
        let mut reader = BufReader::new(input);
        let mut out = Vec::new();
        loop {
            let item = read_line_limited(&mut reader, max).await;
            let stop = !matches!(item, Ok(Some(_)));
            out.push(item);
            if stop {
                return out;
            }
        }
    }

    #[tokio::test]
    async fn two_lines_then_eof() {
        let items = read_all(b"one\ntwo\n", 100).await;
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_ref().unwrap().as_deref(), Some("one"));
        assert_eq!(items[1].as_ref().unwrap().as_deref(), Some("two"));
        assert_eq!(items[2].as_ref().unwrap(), &None);
    }

    #[tokio::test]
    async fn crlf_is_trimmed_and_empty_line_is_a_line() {
        let items = read_all(b"a\r\n\n", 100).await;
        assert_eq!(items[0].as_ref().unwrap().as_deref(), Some("a"));
        assert_eq!(items[1].as_ref().unwrap().as_deref(), Some(""));
    }

    #[tokio::test]
    async fn line_over_the_limit_is_invalid_data() {
        let items = read_all(b"0123456789\n", 5).await;
        assert_eq!(
            items[0].as_ref().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let exact = read_all(b"01234\n", 5).await;
        assert_eq!(exact[0].as_ref().unwrap().as_deref(), Some("01234"));
    }

    #[tokio::test]
    async fn limit_holds_without_a_newline() {
        let long = vec![b'x'; 10_000];
        let items = read_all(&long, 100).await;
        assert_eq!(
            items[0].as_ref().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn disconnect_in_the_middle_of_a_line_is_unexpected_eof() {
        let items = read_all(b"half", 100).await;
        assert_eq!(
            items[0].as_ref().unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn invalid_utf8_is_invalid_data() {
        let items = read_all(b"\xff\xfe\n", 100).await;
        assert_eq!(
            items[0].as_ref().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn line_split_across_writes_is_assembled() {
        let (mut tx, rx) = duplex(4);
        let writer = tokio::spawn(async move {
            for part in ["he", "llo ", "wor", "ld\n"] {
                tx.write_all(part.as_bytes()).await.unwrap();
            }
        });
        let mut reader = BufReader::new(rx);
        let line = read_line_limited(&mut reader, 100).await.unwrap();
        assert_eq!(line.as_deref(), Some("hello world"));
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn write_line_adds_the_newline_once() {
        let mut out = Vec::new();
        write_line(&mut out, "a").await.unwrap();
        write_line(&mut out, "b\n").await.unwrap();
        assert_eq!(out, b"a\nb\n");
    }
}
