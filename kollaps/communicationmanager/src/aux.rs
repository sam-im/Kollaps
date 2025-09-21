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

use std::borrow::BorrowMut;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use capnp::message::{Builder, HeapAllocator};
use capnp::serialize_packed;
use tracing::debug;

pub struct Service {
    pub id: String,
    pipe: File,
}

impl Service {
    pub fn write(&mut self, msg: &Builder<HeapAllocator>) {
        serialize_packed::write_message(self.pipe.borrow_mut(), msg).unwrap();
    }
}

// receives the ids and creates a structure with all the pipes to read from
pub fn get_services(
    local_ids: &Vec<String>,
    pathread: &str,
) -> io::Result<Vec<Arc<Mutex<Service>>>> {
    let mut services = vec![];

    for id in local_ids {
        let path = format!("{}{}", pathread, id);
        let file = OpenOptions::new().write(true).open(&path)?;

        let service = Service {
            pipe: file,
            id: id.to_owned(),
        };

        let service = Arc::new(Mutex::new(service));

        services.push(service);
    }

    Ok(services)
}

/// Messages used in setup, i.e. while each each host waits all of the services to start.
pub enum SetupMessage {
    ServiceCount(u16),
    Terminate,
}

impl From<[u8; 3]> for SetupMessage {
    fn from(value: [u8; 3]) -> Self {
        match value {
            [_, _, 1] => Self::Terminate,
            [lo, hi, _] => {
                let n = (u16::from(hi) << 8) | (u16::from(lo));
                SetupMessage::ServiceCount(n)
            }
        }
    }
}

impl From<SetupMessage> for [u8; 3] {
    fn from(msg: SetupMessage) -> Self {
        match msg {
            SetupMessage::Terminate => [0, 0, 1],
            SetupMessage::ServiceCount(n) => {
                let lo = (n & 0xFF) as u8;
                let hi = (n >> 8) as u8;
                [lo, hi, 0]
            }
        }
    }
}

/// Helper function that polls for the existence of a file in a given `path`.
///
/// If the `timeout` is not set and the file never exists, the thread will hang indefinitely.
///
/// Setting the `read` argument to true will read the file contents and returns it inside `Option::Some`.
pub fn wait_for_file<P: AsRef<Path>>(
    path: P,
    timeout: Option<Duration>,
    read: bool,
) -> io::Result<Option<Vec<String>>> {
    let path = path.as_ref();
    let start = Instant::now();
    let interval = Duration::from_secs(1);

    loop {
        if path.exists() {
            break;
        }
        if let Some(max) = timeout
            && start.elapsed() >= max
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Timed out waiting for file: {}", path.display()),
            ));
        }
        debug!(
            "File {} does not exist, sleeping for {:?}.",
            path.display(),
            interval
        );
        sleep(Duration::from_secs(1));
    }

    if read {
        let lines = read_lines(path)?;
        return Ok(Some(lines));
    }
    Ok(None)
}

pub fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    let file = OpenOptions::new().read(true).open(path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();
    Ok(lines)
}
