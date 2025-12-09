use std::env::args;
use std::io::{copy, sink};
use std::net::TcpListener;

fn main() {
    eprintln!("Starting server.");
    let mut args = args();
    let _prog = args.next();
    let port = args
        .next()
        .and_then(|a| a.parse::<u32>().ok())
        .unwrap_or(3000);
    let addr = format!("0.0.0.0:{}", port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        }
    };

    eprintln!("Listening on {}", addr);

    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let peer_addr = s
                .peer_addr()
                .and_then(|a| Ok(a.to_string()))
                .unwrap_or("<failed to get peer address>".to_string());
            let mut sink = sink();
            if let Err(e) = copy(&mut s, &mut sink) {
                eprintln!("Error while reading from client {}: {}", peer_addr, e);
            }
            eprintln!("Stream for {} dropped.", peer_addr);
        }
    }
}
