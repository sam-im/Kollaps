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

use std::sync::OnceLock;

use docker_api::opts::{ExecStartOpts, ExecCreateOpts};
use docker_api::Exec;
use subprocess::PopenConfig;
use subprocess::Popen;
use subprocess::Redirection;

static COMMAND_STRING: OnceLock<String> = OnceLock::new();

pub async fn start_experiment(id:String){
    get_command_string(id.clone()).await;

    let docker = docker_api::Docker::unix("/var/run/docker.sock");
    let container = docker.containers().get(id);

    let command_string;
    if COMMAND_STRING.get().map_or_else(|| true, |s| s.is_empty()) {
        println!("Leaving start experiment nothing to do");
        return;
    } else {
        command_string = COMMAND_STRING
            .get()
            .unwrap()
            .replace('"', "\"")
            .replace("'", "\\'");
    }

    let mut args = vec![];

    args.push("/bin/sh");
    args.push("-c");
 
    args.push(&command_string);

    // Create Opts with specified command
    let opts = ExecCreateOpts::builder()
        .command(args)
        .attach_stdout(true)
        .attach_stderr(true)
        .build();

    let exec = Exec::create(docker, &container.id(), &opts).await.unwrap();

    exec.start(&ExecStartOpts::builder().detach(false).build())
        .await
        .unwrap();
    //println!("{:#?}", exec.as_ref().unwrap().inspect().await);
}

//Retrieve the command to run
pub async fn get_command_string(id:String){
    let docker = docker_api::Docker::unix("/var/run/docker.sock");

    let container = docker.containers().get(id);
    
    let mut command_string = "".to_string();
    match container.inspect().await {
        Ok(container) => {
            let container_config = container.config.unwrap();

            let image = container_config.image;

            let docker_image = docker.images().get(image.unwrap());

            let mut command = vec![];
            match docker_image.inspect().await{
                Ok(image) =>{
                    let image_config = image.config.as_ref();

                    let entrypoint = image_config.unwrap().entrypoint.clone();

                    if entrypoint.is_none(){
                        return
                    }

                    if container_config.cmd.is_none(){
                        if !image_config.unwrap().cmd.is_none(){
                            command.extend(image_config.unwrap().cmd.as_ref().unwrap().clone());
                        }
                    }

                    else if container_config.cmd.as_ref().unwrap().len() == 0{
                        if !image_config.unwrap().cmd.is_none(){
                            command.extend(image_config.unwrap().cmd.as_ref().unwrap().clone());
                        }
                    }else{
                        command.extend(container_config.cmd.as_ref().unwrap().clone());
                    }
                    
                    for string in entrypoint.unwrap(){
                        command_string = format!("{} {}",command_string,string);
                    }
                    for string in command{
                        command_string = format!("{} {}",command_string,string);
                    }
                    

                    
                },
                Err(e) => eprintln!("Error in command string: {}", e),
            }
        },
        Err(e) => eprintln!("Error in command_string: {}", e),
    };

    COMMAND_STRING.get_or_init(|| command_string);
}

//Kill every process in the namespace of the container
pub fn stop_experiment(pid:u32,code:u32){
    //If it is a kill
    if code==3{
        Popen::create(&["nsenter", "-t", &pid.to_string(),"-p","-m","/bin/sh","-c","kill -9 -1"], PopenConfig {
            stdout: Redirection::Pipe, ..Default::default()
        }).unwrap();
    }
    //If it is a leave
    if code==2{
        Popen::create(&["nsenter", "-t", &pid.to_string(),"-p","-m","/bin/sh","-c","kill -2 -1"], PopenConfig {
            stdout: Redirection::Pipe, ..Default::default()
        }).unwrap();
    }

}
