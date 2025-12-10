use std::env::args;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

const DEFAULT_PORT: u16 = 3000;

fn main() {
    eprintln!("Starting client.");
    let mut args = args();
    let _prog = args.next();
    let addr = match args
        .next()
        .and_then(|a| Ipv4Addr::from_str(a.as_ref()).ok())
    {
        Some(a) => a,
        None => {
            eprintln!(
                "Usage: required first positional argument: IPv4 address, optional second positional argument: port (defaults to {})",
                DEFAULT_PORT
            );
            return;
        }
    };
    let port: u16 = args
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let addr = SocketAddr::new(IpAddr::V4(addr), port);

    let buf = vec![1; 1024];
    let mut stream = connect_with_retry(addr);
    let mut _total_sent: u64 = 0;
    loop {
        match stream.write_all(&buf) {
            Ok(_) => {
                _total_sent += buf.len() as u64;
            },
            Err(e) => {
                eprintln!("{}", e);
                stream = connect_with_retry(addr);
                continue;
            },
        }
    }
}

fn connect_with_retry(addr: SocketAddr) -> TcpStream {
    eprint!("Attempting to connect to {}", addr);
    loop {
        match TcpStream::connect(addr) {
            Ok(stream) => {
                eprintln!("Connected to {}", addr);
                return stream;
            }
            Err(_) => {
                eprintln!("Failed to connect to {}. Retrying in 1s...", addr);
                sleep(Duration::from_secs(1));
            }
        }
    }
}
