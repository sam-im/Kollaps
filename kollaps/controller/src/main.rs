// Licensed to the Apache Software Foundation (ASF) under one or more
// contributor license agreements.  See the NOTICE file distributed with
// this work for additional information regarding copyright ownership.
// The ASF licenses this file to You under the Apache License, Version 2.0
// (the "License"); you may not use this file except in compliance with
// the License.  You may obtain a copy of the License at

//    http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use emulationcore::xmlgraphparser::XMLGraphParser;
use std::env;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::sleep;

const SHUTDOWN_CMD: u8 = 2;
const READY_CMD: u8 = 3;
const START_CMD: u8 = 4;
const SLEEP_DURATION: Duration = Duration::from_millis(1000);

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() -> Result<()> {
    let topology_file = env::args().nth(1).unwrap();
    let command = env::args().nth(2).unwrap();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(1)
        .build()?;

    rt.block_on(process_command(topology_file, command))?;

    Ok(())
}

async fn process_command(topology_file: String, command: String) -> Result<()> {
    if command == "ready" {
        let text = std::fs::read_to_string(topology_file)?;

        let parser = XMLGraphParser::try_new(&text, "baremetal".to_string()).expect("valid xml topology file");
        let (config, _) = parser.fill_graph().await;

        let mut remote_ips = vec![];
        let ips = config.ips.clone();

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open("/ips.txt")
            .await?;

        for ip in ips {
            let ip_with_port = if ip == config.controller_ip {
                format!("0.0.0.0:{}", "7073")
            } else {
                format!("{}:{}", ip, "7073")
            };
            remote_ips.push(ip_with_port.clone());
            file.write_all(format!("{}\n", ip_with_port).as_bytes())
                .await?;
        }

        let mut streams = vec![];
        let mut ips_connected = vec![];
        while ips_connected.len() != remote_ips.len() {
            sleep(SLEEP_DURATION).await;
            for (i, remote_ip) in remote_ips.iter().enumerate() {
                if !(ips_connected.contains(&i)) {
                    let stream = TcpStream::connect(remote_ip).await;
                    match stream {
                        Ok(stream) => {
                            streams.push(stream);
                            ips_connected.push(i);
                        }
                        Err(e) => println!("{}", e.to_string()),
                    };
                }
            }
        }

        let mut buffer = vec![0; 1];

        buffer[0] = READY_CMD;

        for mut stream in streams {
            stream.write_all(&buffer).await?;
        }
    }

    if command == "start" {
        let pathremoteips = "/ips.txt";

        let file = OpenOptions::new().read(true).open(&pathremoteips).await?;

        let reader = BufReader::new(file);

        let mut lines = reader.lines();

        let mut remote_ips: Vec<String> = vec![];

        while let Ok(Some(line)) = lines.next_line().await {
            remote_ips.push(line);
        }

        let mut streams = vec![];
        let mut ips_connected = vec![];
        while ips_connected.len() != remote_ips.len() {
            sleep(SLEEP_DURATION).await;
            for (i, remote_ip) in remote_ips.iter().enumerate() {
                if !(ips_connected.contains(&i)) {
                    let stream = TcpStream::connect(remote_ip).await;
                    match stream {
                        Ok(stream) => {
                            streams.push(stream);
                            ips_connected.push(i);
                        }
                        Err(e) => println!("{}", e.to_string()),
                    };
                }
            }
        }

        let mut buffer = vec![0; 1];

        buffer[0] = START_CMD;

        for mut stream in streams {
            stream.write(&buffer).await?;
        }
    }

    if command == "stop" {
        let pathremoteips = "/ips.txt";

        let file = OpenOptions::new().read(true).open(&pathremoteips).await?;

        let reader = BufReader::new(file);

        let mut lines = reader.lines();

        let mut remote_ips: Vec<String> = vec![];

        while let Ok(Some(line)) = lines.next_line().await {
            remote_ips.push(line);
        }

        let mut streams = vec![];
        let mut ips_connected = vec![];
        while ips_connected.len() != remote_ips.len() {
            sleep(SLEEP_DURATION).await;
            for (i, remote_ip) in remote_ips.iter().enumerate() {
                if !(ips_connected.contains(&i)) {
                    let stream = TcpStream::connect(remote_ip).await;
                    match stream {
                        Ok(stream) => {
                            streams.push(stream);
                            ips_connected.push(i);
                        }
                        Err(e) => println!("{}", e.to_string()),
                    };
                }
            }
        }

        let mut buffer = vec![0; 1];

        buffer[0] = SHUTDOWN_CMD;

        for mut stream in streams {
            stream.write(&buffer).await?;
        }
    }

    Ok(())
}
