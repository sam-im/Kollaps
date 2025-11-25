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
use std::sync::OnceLock;
use std::thread;

const READ_PIPE_PATH: &str = "/tmp/kollaps/pipes/piperead";
const WRITE_PIPE_PATH: &str = "/tmp/kollaps/pipes/pipewrite";

static PIPES: OnceLock<Pipes> = OnceLock::new();
static DASHBOARD: OnceLock<PyObject> = OnceLock::new();

struct Pipes {
    _write_pipe: File,
    read_pipe: File,
}

// Exports functions to the Python module.
#[pymodule]
fn libcommunicationcore(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(start))?;
    m.add_wrapped(wrap_pyfunction!(register_communicationmanager))?;
    m.add_wrapped(wrap_pyfunction!(start_polling_u16))?;

    Ok(())
}

// Creates pipes belonging to the Dashboard service.
#[pyfunction]
fn start(_py: Python, id: String, _name: String, _ip: u32, _link_count: u32) -> PyResult<()> {
    println!("Dashboard (communicationcore id {id}): starting.");

    let read_path = format!("{}{}", READ_PIPE_PATH, id);
    let c_path = CString::new(read_path.clone())?;
    unsafe {
        libc::mkfifo(c_path.as_ptr(), 0o644);
    }

    let write_path = format!("{}{}", WRITE_PIPE_PATH, id);
    let c_path = CString::new(write_path.clone())?;
    unsafe {
        libc::mkfifo(c_path.as_ptr(), 0o644);
    }
    let read_pipe = OpenOptions::new()
        .read(true)
        .open(read_path)?;

    let _write_pipe = OpenOptions::new()
        .write(true)
        .open(write_path)?;

    let communication = Pipes { read_pipe, _write_pipe };
    PIPES.get_or_init(|| communication);
    println!("Dashboard (communicationcore id {id}): startup ended.");
    Ok(())
}

// Saves a reference to the Python object, used as a callback.
#[pyfunction]
fn register_communicationmanager(objectpython: PyObject) -> PyResult<()> {
    DASHBOARD.get_or_init(|| objectpython);
    Ok(())
}

// Receives traffic metadata from other services.
#[pyfunction]
fn start_polling_u16() -> PyResult<()> {
    thread::spawn(move || {
        let pipes = PIPES.get().unwrap();
        let mut buf_reader = BufReader::new(&pipes.read_pipe);

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

// Sends metadata to the Dashboard using the reference to the Python object.
fn callreceive_flow_16(bandwidth: u32, link_count: u16, ids: Vec<u16>) {
    Python::with_gil(|py| {
        let dashboard_py = DASHBOARD
            .get()
            .expect("Dashboard (communicationcore): python object must have been initialized.");

        dashboard_py
            .call_method(py, "receive_flow", (bandwidth, link_count, ids), None)
            .map_err(|err| eprintln!("Dashboard (communicationcore): {:?}", err))
            .ok();
    });
}
