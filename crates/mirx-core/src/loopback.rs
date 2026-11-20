//! The listener the homeserver's sign-in page sends the browser back to:
//! one port on 127.0.0.1, for one sign-in, at an unguessable path.

use std::io;
use std::net::Ipv4Addr;
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use matrix_sdk::authentication::oauth::CsrfToken;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

/// A browser that opens a connection and sends nothing within this long
/// is dropped; others are served meanwhile.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUEST_HEAD: usize = 8 * 1024;

pub(crate) struct LoopbackListener {
    listener: TcpListener,
    redirect_uri: Url,
}

/// The browser's request for the redirect URI, held open to tell the user
/// how the sign-in ended.
#[derive(Debug)]
pub(crate) struct LoopbackRedirect {
    pub(crate) stream: TcpStream,
    /// The query of the request, without the `?`; empty if it had none.
    pub(crate) query: String,
}

impl LoopbackListener {
    /// Listens on a free port of 127.0.0.1 only: the redirect carries the
    /// sign-in's code or token, which must not leave the machine. The
    /// literal address, not `localhost`, as RFC 8252 asks: `localhost` may
    /// resolve to `::1`.
    pub(crate) async fn bind() -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        // Legacy SSO redirects carry no state to check, so the path is
        // the secret that keeps another local page from handing mirx a
        // login token of its choosing. The token is URL-safe base64.
        let path = CsrfToken::new_random().secret().clone();
        let redirect_uri = Url::parse(&format!("http://127.0.0.1:{port}/{path}"))
            .expect("a loopback URL with a base64url path parses");
        Ok(Self {
            listener,
            redirect_uri,
        })
    }

    pub(crate) fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
    }

    /// Waits for the request for the redirect URI, answering every other
    /// one (a favicon, a stray page) with an error; the port closes when
    /// this returns.
    pub(crate) async fn next_redirect(self) -> io::Result<LoopbackRedirect> {
        let mut requests = FuturesUnordered::new();
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    let (stream, _) = accepted?;
                    requests.push(read_request_head(stream));
                }
                Some((mut stream, head)) = requests.next() => {
                    let Some(target) = head.as_deref().and_then(request_target) else {
                        let _ = respond(&mut stream, "400 Bad Request", "").await;
                        continue;
                    };
                    let (path, query) = target.split_once('?').unwrap_or((target, ""));
                    if path != self.redirect_uri.path() {
                        // A stray request's answer is a courtesy.
                        let _ = respond(&mut stream, "404 Not Found", "").await;
                        continue;
                    }
                    return Ok(LoopbackRedirect {
                        stream,
                        query: query.to_owned(),
                    });
                }
            }
        }
    }
}

/// The request head, up to the blank line, or `None` if the browser sent
/// none in time, or too much, or not text.
async fn read_request_head(mut stream: TcpStream) -> (TcpStream, Option<String>) {
    let head = tokio::time::timeout(REQUEST_TIMEOUT, async {
        let mut head = Vec::new();
        let mut buffer = [0; 1024];
        while !head.ends_with(b"\r\n\r\n") {
            let read = stream.read(&mut buffer).await.ok()?;
            if read == 0 || head.len() + read > MAX_REQUEST_HEAD {
                return None;
            }
            head.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8(head).ok()
    })
    .await
    .ok()
    .flatten();
    (stream, head)
}

/// The target of a `GET` request line, such as `/path?code=1&state=2`.
fn request_target(head: &str) -> Option<&str> {
    let request_line = head.lines().next()?;
    match request_line.split(' ').collect::<Vec<_>>()[..] {
        ["GET", target, "HTTP/1.1" | "HTTP/1.0"] if target.starts_with('/') => Some(target),
        _ => None,
    }
}

/// Answers the browser and closes the connection.
pub(crate) async fn respond(stream: &mut TcpStream, status: &str, html: &str) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

/// The page the browser shows once the sign-in is over.
pub(crate) fn result_page(text: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>mirx</title></head><body><p>{}</p></body></html>\n",
        html_escape(text)
    )
}

fn html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;

    #[test]
    fn request_target_takes_get_requests_only() {
        assert_eq!(
            request_target("GET /?code=1&state=2 HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some("/?code=1&state=2")
        );
        assert_eq!(
            request_target("GET /favicon.ico HTTP/1.1\r\n\r\n"),
            Some("/favicon.ico")
        );
        for head in [
            "POST /?code=1 HTTP/1.1\r\n\r\n",
            "GET http://127.0.0.1/ HTTP/1.1\r\n\r\n",
            "GET / HTTP/2\r\n\r\n",
            "GET /\r\n\r\n",
            "garbage",
            "",
        ] {
            assert_eq!(request_target(head), None, "{head:?}");
        }
    }

    #[test]
    fn result_pages_escape_their_text() {
        assert!(result_page("<b>&\"'</b>").contains("<p>&lt;b&gt;&amp;&quot;&#39;&lt;/b&gt;</p>"));
    }

    /// Sends `request` as a browser would, and returns the whole response.
    async fn browse(port: u16, request: String) -> String {
        tokio::task::spawn_blocking(move || {
            let mut stream = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                .expect("connect to the listener");
            stream.write_all(request.as_bytes()).expect("send request");
            let mut response = String::new();
            stream.read_to_string(&mut response).expect("read response");
            response
        })
        .await
        .expect("the browser thread")
    }

    #[test]
    fn next_redirect_answers_other_requests_and_returns_the_redirect() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        runtime.block_on(async {
            let listener = LoopbackListener::bind().await.expect("bind listener");
            let redirect_uri = listener.redirect_uri().clone();
            assert_eq!(redirect_uri.host_str(), Some("127.0.0.1"));
            let port = redirect_uri.port().expect("the redirect names its port");
            let path = redirect_uri.path().to_owned();
            let next_redirect = tokio::spawn(listener.next_redirect());

            // An idle connection does not hold up the others.
            let idle = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                .expect("connect to the listener");
            for request in [
                "GET /favicon.ico HTTP/1.1\r\n\r\n",
                "GET /?code=1&state=2 HTTP/1.1\r\n\r\n",
            ] {
                let response = browse(port, request.to_owned()).await;
                assert!(
                    response.starts_with("HTTP/1.1 404 Not Found\r\n"),
                    "{response}"
                );
            }
            let response = browse(port, "POST / HTTP/1.1\r\n\r\n".to_owned()).await;
            assert!(
                response.starts_with("HTTP/1.1 400 Bad Request\r\n"),
                "{response}"
            );
            assert!(!next_redirect.is_finished());

            let browser = tokio::spawn(browse(
                port,
                format!("GET {path}?code=1&state=2 HTTP/1.1\r\nHost: x\r\n\r\n"),
            ));
            let mut redirected = next_redirect
                .await
                .expect("the listener task")
                .expect("the redirect");
            assert_eq!(redirected.query, "code=1&state=2");
            respond(&mut redirected.stream, "200 OK", &result_page("done"))
                .await
                .expect("answer the browser");
            let response = browser.await.expect("the browser task");
            assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
            assert!(
                response.ends_with("<p>done</p></body></html>\n"),
                "{response}"
            );
            drop(idle);
            // The port closed with the listener.
            assert!(std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
        });
    }
}
