//! Offline integration tests for the public HTTP contract.
use llamancer::llama::client::Client;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

#[test]
fn router_snapshot_load_and_unload() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for (path, response) in [
            ("/health", r#"{"status":"ok"}"#),
            ("/props", r#"{"role":"router"}"#),
            (
                "/models",
                r#"{"data":[{"id":"example","status":{"value":"unloaded"}}]}"#,
            ),
            ("/models/load", r#"{"success":true}"#),
            ("/models/unload", r#"{"success":true}"#),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.split_whitespace().nth(1), Some(path));
            let mut len = 0;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        len = value.trim().parse().unwrap();
                    }
                }
            }
            if path.ends_with("load") {
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(value["model"], "example");
            }
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
        }
    });
    let client = Client::new(&endpoint).unwrap();
    let snapshot = client.snapshot().unwrap();
    assert!(snapshot.router);
    assert_eq!(snapshot.models[0].state(), "unloaded");
    client.set_loaded("example", true).unwrap();
    client.set_loaded("example", false).unwrap();
    server.join().unwrap();
}
