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

use capnp::serialize_packed;
use capnp_schemas::message_capnp::message;
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use std::ffi::CString;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::thread;

// pipe to write to RM, pipe to read from RM, pipe to read local usage from RM
struct Communication {
    writepipe: Option<File>,
    readpipe: Option<File>,
    _buf_reader: Option<BufReader<File>>,
}

// docker container id
static CONTAINERID: OnceLock<String> = OnceLock::new();

// docker container name
static CONTAINERNAME: OnceLock<String> = OnceLock::new();

// ip of container in int (smallendian)
static CONTAINERIP: OnceLock<u32> = OnceLock::new();

// limit of links in topology
static CONTAINERLIMIT: OnceLock<u32> = OnceLock::new();

// init global
static COMMUNICATION: LazyLock<Mutex<Communication>> = LazyLock::new(|| {
    Mutex::new(Communication {
        writepipe: None,
        readpipe: None,
        _buf_reader: None,
    })
});

// python reference
static COMMUNICATIONMANAGER: OnceLock<PyObject> = OnceLock::new();

// python module definitions
#[pymodule]
fn libcommunicationcore(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(start))?;
    m.add_wrapped(wrap_pyfunction!(register_communicationmanager))?;
    m.add_wrapped(wrap_pyfunction!(start_polling_u16))?;

    Ok(())
}

/**********************************************************************************************
*       lib functions
**********************************************************************************************/

#[pyfunction]
fn start(_py: Python, id: String, name: String, ip: u32, link_count: u32) -> PyResult<()> {
    CONTAINERID.get_or_init(|| id.clone());
    CONTAINERNAME.get_or_init(|| name.clone());
    CONTAINERIP.get_or_init(|| ip);
    CONTAINERLIMIT.get_or_init(|| if link_count <= 255 { 8 } else { 16 });

    //create files
    let pathwrite = "/tmp/pipewrite";
    let pathread = "/tmp/piperead";
    let pathlocal = "/tmp/pipelocal";

    let pathread = format!("{}{}", pathread, id.to_string());
    let filename = CString::new(pathread.clone()).unwrap();
    unsafe {
        libc::mkfifo(filename.as_ptr(), 0o644);
    }

    let pathwrite = format!("{}{}", pathwrite, id.to_string());
    let filename = CString::new(pathwrite.clone()).unwrap();
    unsafe {
        libc::mkfifo(filename.as_ptr(), 0o644);
    }

    let pathlocal = format!("{}{}", pathlocal, id.to_string());
    let filename = CString::new(pathlocal.clone()).unwrap();
    unsafe {
        libc::mkfifo(filename.as_ptr(), 0o644);
    }

    //collect pipe for reading
    print_message("GETTING READ PIPE");
    let fileread = OpenOptions::new()
        .read(true)
        .open(pathread)
        .expect("file not found");
    print_message("GOT READ PIPE");

    //collect pipe for writing
    print_message("GETTING WRITE PIPE");
    let filewrite = OpenOptions::new()
        .write(true)
        .open(pathwrite)
        .expect("file not found");
    print_message("GOT WRITE PIPE");

    let mut communication = COMMUNICATION.lock().unwrap();
    communication.readpipe = Some(fileread);
    communication.writepipe = Some(filewrite);

    Ok(())
}

// save reference to python
#[pyfunction]
fn register_communicationmanager(objectpython: PyObject) -> PyResult<()> {
    COMMUNICATIONMANAGER.get_or_init(|| objectpython);
    Ok(())
}

// start reading information from RM related to flows from other containers
#[pyfunction]
fn start_polling_u16() -> PyResult<()> {
    let _handle = thread::spawn(move || {
        let communication = COMMUNICATION.lock().unwrap();
        let mut buf_reader = BufReader::new(communication.readpipe.as_ref().unwrap());

        loop {
            let message_reader = serialize_packed::read_message(
                &mut buf_reader,
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();

            let message = message_reader.get_root::<message::Reader>().unwrap();

            let flows = message.get_flows().unwrap();

            for flow in flows {
                let bandwidth = flow.get_bw();
                let links = flow.get_links().unwrap();
                let link_count = links.len() as u16;

                let mut ids = vec![];

                for i in 0..link_count {
                    ids.push(links.get(i as u32).get_id());
                }

                callreceive_flow_16(bandwidth, link_count, ids);
            }
        }
    });
    Ok(())
}

// call python to give information about flows from other containers
fn callreceive_flow_16(bandwidth: u32, link_count: u16, ids: Vec<u16>) {
    Python::with_gil(|py| {
        let commsmanager = COMMUNICATIONMANAGER
            .get()
            .expect("communicationcore: communicationmanager must have been initialized");

        commsmanager
            .call_method(py, "receive_flow", (bandwidth, link_count, ids), None)
            .map_err(|err| println!("communicationcore: {:?}", err))
            .ok();
    });
}

fn print_message(message_to_print: &str) {
    let container_name = CONTAINERNAME.get_or_init(|| "containername not initialized".to_string());
    let message = format!(
        "communicationcore - {} : {}",
        container_name, message_to_print
    );

    println!("{}", message);
}
