//! Headless check of the engine loop: spawn it against a local fixture server
//! and require a navigated status plus a nonblank frame. Runs the real V8 +
//! render pipeline in this test process, so run it under nextest.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use telemaco_gui::engine::{self, EngineOptions, Update};

fn spawn_fixture() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback fixture");
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let body = "<html><head><title>GUI smoke</title></head>\
                    <body style=\"background:#c0392b;margin:0\">\
                    <div style=\"width:300px;height:300px;background:#2980b9\"></div>\
                    </body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        for stream in listener.incoming() {
            if let Ok(mut socket) = stream {
                let mut buffer = [0u8; 4096];
                let _ = socket.read(&mut buffer);
                let _ = socket.write_all(response.as_bytes());
                let _ = socket.flush();
            }
        }
    });
    format!("http://{addr}/")
}

#[test]
fn engine_navigates_and_streams_a_nonblank_frame() {
    std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
    let url = spawn_fixture();
    let (tx, rx) = engine::spawn(EngineOptions {
        start_url: Some(url.clone()),
        ..Default::default()
    });

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut saw_frame = false;
    let mut saw_status = false;
    let mut viewport_applied = false;
    let mut viewport_ok = false;
    while Instant::now() < deadline && !(saw_status && viewport_ok) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(engine::Update::Frame { rgba, width, height }) => {
                saw_frame = true;
                assert!(width > 0 && height > 0, "frame has a size");
                assert_eq!(
                    rgba.len(),
                    (width as usize) * (height as usize) * 4,
                    "frame buffer does not match its dimensions"
                );
                // The device-scale override must size the bitmap at
                // CSS viewport * scale (500 x 400 @ 2x -> 1000 x 800).
                if viewport_applied && (width, height) == (1000, 800) {
                    viewport_ok = true;
                } else if !viewport_applied {
                    // Exercise the resize path once, after the first frame.
                    let _ = tx.send(engine::Command::Viewport {
                        width: 500,
                        height: 400,
                        scale: 2.0,
                    });
                    viewport_applied = true;
                }
                let mut distinct = 0usize;
                let mut last: Option<[u8; 3]> = None;
                for pixel in rgba.chunks_exact(4).step_by(8192) {
                    let current = [pixel[0], pixel[1], pixel[2]];
                    if last != Some(current) {
                        last = Some(current);
                        distinct += 1;
                    }
                    if distinct > 1 {
                        break;
                    }
                }
                assert!(distinct > 1, "captured frame is blank");
            }
            Ok(engine::Update::Status { url: got, loading, .. }) => {
                if !loading && got.starts_with("http") {
                    saw_status = true;
                    assert!(
                        got.starts_with(&url),
                        "status url {got} does not match fixture {url}"
                    );
                }
            }
            Ok(engine::Update::Error(message)) => panic!("engine error: {message}"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("update channel failed: {error:?}"),
        }
    }

    let _ = tx.send(engine::Command::Shutdown);
    assert!(saw_frame, "no frame arrived before the deadline");
    assert!(saw_status, "no loaded status arrived before the deadline");
    assert!(viewport_ok, "viewport override was never honored by a frame");
}