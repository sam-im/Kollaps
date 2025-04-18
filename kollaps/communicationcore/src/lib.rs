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
use std::fs::OpenOptions;
use std::fs::File;
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use std::thread;
use std::ffi::CString;
use capnp::serialize_packed;
use capnp_schemas::message_capnp::message;
use std::io::BufReader;
use std::sync::{OnceLock, LazyLock, Mutex};

pub struct FdSet(libc::fd_set);

//docker container id
static CONTAINERID: OnceLock<String> = OnceLock::new();

//docker container name
static CONTAINERNAME: OnceLock<String> = OnceLock::new();

//ip of container in int (smallendian)
static CONTAINERIP: OnceLock<u32> = OnceLock::new();

//limit of links in topology
static CONTAINERLIMIT: OnceLock<u32> = OnceLock::new();

//pipe to write to RM, pipe to read from RM, pipe to read local usage from RM
struct Communication {
    writepipe: Option<File>,
    readpipe: Option<File>,
    buf_reader: Option<BufReader<File>>
}

//struct of messages to RM
struct Message{
    content: Vec<u8>,
    index: usize,
}

//init global
static COMMUNICATION: LazyLock<Mutex<Communication>> = LazyLock::new(|| {
    Mutex::new(Communication {
        writepipe: None,
        readpipe: None,
        buf_reader: None,
    })
});

//init message (always the same struct all function write to and read from this)
static MESSAGE: LazyLock<Mutex<Message>> = LazyLock::new(|| {
    Mutex::new(Message {
        content: Vec::new(),
        index: 0  //where the to start writing information to
    })
});

//python reference
static COMMUNICATIONMANAGER: OnceLock<PyObject> = OnceLock::new();


//python module definitions
#[pymodule]
fn libcommunicationcore(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(start, m)?)?;
    m.add_function(wrap_pyfunction!(register_communicationmanager, m)?)?;
    m.add_function(wrap_pyfunction!(start_polling_u8, m)?)?;
    m.add_function(wrap_pyfunction!(start_polling_u16, m)?)?;


    Ok(())
}

/**********************************************************************************************
*       buffer and different sized ints
**********************************************************************************************/


fn put_uint16(index:usize,value:u32){
    let mut message = MESSAGE.lock().unwrap();
    message.content[index] = ((value >> 0) & 0xff) as u8;
    message.content[index+1] = ((value >> 8) & 0xff) as u8;
}

/*fn put_uint32_in_buffer(value:u32) -> [u8;4]{

    let mut buffer = [0;4];
    buffer[0] = ((value >> 0) & 0xff) as u8;
    buffer[1] = ((value >> 8) & 0xff)  as u8;
    buffer[2] = ((value >> 16) & 0xff) as u8;
    buffer[3] = ((value >> 24)& 0xff) as u8;
    return buffer;
}*/

/*fn get_uint8(buffer:&Vec<u8>,index:usize)-> u32{

    return ((buffer[index] >> 0) & 0xff) as u32;

}*/

fn get_uint16(buffer:&Vec<u8>,index:usize) -> u16{
    return (u16::from(buffer[index+1]) << 8) | (u16::from(buffer[index])) ;

}


fn get_uint32(buffer:&Vec<u8>,index:usize) -> u32{

    return (u32::from(buffer[index+3]) << 24) | 
            (u32::from(buffer[index+2]) << 16) | 
            (u32::from(buffer[index+1]) << 8)  |
            (u32::from(buffer[index])) ;
}


fn init_content(){
    let mut message = MESSAGE.lock().unwrap();
    message.content.resize(512, 0);
}


/**********************************************************************************************
*       lib functions
**********************************************************************************************/


#[pyfunction]
//start the lib
fn start(_py: Python,id: String,name:String,ip:u32,link_count:u32){
    CONTAINERID.get_or_init(|| id.clone());
    CONTAINERNAME.get_or_init(|| name.clone());
    CONTAINERIP.get_or_init(|| ip);
    CONTAINERLIMIT.get_or_init(|| {
        match link_count {
            n if n <= 255 => 8,
            _ => 16
        }
    });

    //create files
    let pathwrite = "/tmp/pipewrite";
    let pathread = "/tmp/piperead";


    let pathread = format!("{}{}",pathread ,id.to_string());


    let filename = CString::new(pathread.clone()).unwrap();
        unsafe {
            libc::mkfifo(filename.as_ptr(), 0o644);
        }

    
    let pathwrite = format!("{}{}",pathwrite,id.to_string());


    let filename = CString::new(pathwrite.clone()).unwrap();
    unsafe {
        libc::mkfifo(filename.as_ptr(), 0o644);
    }


    let pathlocal = "/tmp/pipelocal";
    let pathlocal = format!("{}{}",pathlocal ,id.to_string());

    let filename = CString::new(pathlocal.clone()).unwrap();
    unsafe {
        libc::mkfifo(filename.as_ptr(), 0o644);
    }

    //collect pipe for reading
    print_message("STARTED GETTING READ PIPE".to_string());
    let fileread = OpenOptions::new().read(true).open(pathread).expect("file not found");
    print_message("GOT READ PIPE".to_string());

    //collect pipe for writing
    print_message("STARTED GETTING WRITE PIPE".to_string());
    let filewrite = OpenOptions::new().write(true).open(pathwrite).expect("file not found");

    print_message("GOT WRITE PIPE".to_string());

    let mut communication = COMMUNICATION.lock().unwrap();
    communication.readpipe = Some(fileread);
    communication.writepipe = Some(filewrite);
}


//save reference to python
#[pyfunction]
fn register_communicationmanager(objectpython:PyObject){
    COMMUNICATIONMANAGER.get_or_init(|| objectpython);
}


//start reading information from RM related to flows from other containers
#[pyfunction]
fn start_polling_u8(){

    let _handle = thread::spawn(move || {
    //buffer to hold data
    let mut _receive_buffer = vec![0;1024];
    loop{

        // unsafe{
        //     COMMUNICATION.readpipe.as_ref().unwrap().read(&mut receive_buffer).map_err(|err| print_message(format!("{:?}", err))).ok();
        // }

        // let mut recv_ptr = 0;


        // let flow_count = receive_buffer[recv_ptr];
        // recv_ptr += 1;

        // //0 flows means the other node left
        // if flow_count == 0{

        // }else{
        //     for _f in 0..flow_count {

        //         let mut ids = Vec::new();
    
        //         let bandwidth = get_uint32(&receive_buffer,recv_ptr);
        //         recv_ptr += 4;
    
        //         let link_count = receive_buffer[recv_ptr] as usize;
        //         recv_ptr += 1;
    
        //         for _l in 0..link_count {
        //             ids.push(receive_buffer[recv_ptr]);
        //             recv_ptr+=1;
        //         }
    
        //          callreceive_flow(bandwidth,link_count,ids);
    
        //     }
        // }


    }
        
    });

}

//same as u8 but for u16
#[pyfunction]
fn start_polling_u16(){

    let _handle = thread::spawn(move || {
        let communication = COMMUNICATION.lock().unwrap();
        let mut buf_reader = BufReader::new(communication.readpipe.as_ref().unwrap());

            loop{
                let message_reader = serialize_packed::read_message(&mut buf_reader, capnp::message::ReaderOptions::new()).unwrap();

                let message = message_reader.get_root::<message::Reader>().unwrap();

                let flows = message.get_flows().unwrap();

                for flow in flows{
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

}



//call python to give information about flows from other containers
fn callreceive_flow(bandwidth:u32, link_count:usize, ids:Vec<u8>){
    let gil = Python::acquire_gil();
    let py = gil.python();
    let commsmanager = COMMUNICATIONMANAGER.get().expect("communicationmanager must have been initialized");

    commsmanager.call_method(py, "receive_flow", (bandwidth, link_count, ids), None)
                .map_err(|err| println!("{:?}", err)).ok();
}

//call python to give information about flows from other containers
fn callreceive_flow_16(bandwidth:u32, link_count:u16, ids:Vec<u16>){
    let gil = Python::acquire_gil();
    let py = gil.python();
    let commsmanager = COMMUNICATIONMANAGER.get().expect("communicationmanager must have been initialized");

    commsmanager.call_method(py,"receive_flow",(bandwidth,link_count,ids),None)
                .map_err(|err| println!("{:?}", err)).ok();
}

fn print_message(message_to_print: String){
    let container_name = CONTAINERNAME.get_or_init(|| "containername not initialized".to_string());
    let message = format!("RUST EC - {} : {}", container_name, message_to_print);

    println!("{}",message);
}
