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

use std::os::unix::io::RawFd;
use std::{io, mem, ptr, time};

use tracing::error;

/// Wrapper around `libc::select` and `libc::fd_set`.
///
/// # Usage
/// Create a new `Select` instance with a list of file descriptors,
/// and poll with `select` to watch for any available readable events.
///
/// ## Example
/// ```
/// let fds = vec![0];
/// let mut select = Select::new(fds);
/// loop {
///     let read_ready_fds = select.select();
///     read_ready_fds.iter().for_each(|fd| dbg!(fd));
/// }
/// ```
pub struct Select {
    fds: Vec<i32>,
    nfds: i32,
    fd_set: FdSet,
    read_ready_fds: Vec<i32>,
}

impl Select {
    pub fn new(fds: Vec<i32>) -> Self {
        // The `nfds` argument of libc::select is set to the largest file descriptor plus one.
        let nfds = fds.iter().max().unwrap_or(&0) + 1;

        let mut fd_set = FdSet::new();
        fds.iter().for_each(|fd| fd_set.set(*fd));

        Self {
            fds,
            nfds,
            fd_set,
            read_ready_fds: vec![],
        }
    }

    /// Returns a slice to a list of file descriptors that are ready to read.
    pub fn select(&mut self) -> &[i32] {
        self.read_ready_fds.clear();

        match select(self.nfds, Some(&mut self.fd_set), None, None, None) {
            Ok(_) => {
                self.fds
                    .iter()
                    .filter(|&fd| self.fd_set.is_set(*fd))
                    .for_each(|fd| self.read_ready_fds.push(*fd));
            }
            Err(e) => {
                error!("select returned an error: {}", e);
            }
        };

        // Adding a file descriptor that is already present in the set is a no-op, and does not produce an error.
        // Upon return, each of the file descriptor sets is modified in place to indicate which file descriptors are currently "ready".
        // Thus, if using select() within a loop, the sets must be reinitialized before each call. (from `man fd_set`)
        self.fds.iter().for_each(|fd| self.fd_set.set(*fd));

        &self.read_ready_fds
    }
}

fn to_fdset_ptr(opt: Option<&mut FdSet>) -> *mut libc::fd_set {
    match opt {
        None => ptr::null_mut(),
        Some(&mut FdSet(ref mut raw_fd_set)) => raw_fd_set,
    }
}

fn to_ptr<T>(opt: Option<&T>) -> *const T {
    match opt {
        None => ptr::null::<T>(),
        Some(p) => p,
    }
}

fn select(
    nfds: libc::c_int,
    readfds: Option<&mut FdSet>,
    writefds: Option<&mut FdSet>,
    errorfds: Option<&mut FdSet>,
    timeout: Option<&libc::timeval>,
) -> io::Result<usize> {
    match unsafe {
        libc::select(
            nfds,
            to_fdset_ptr(readfds),
            to_fdset_ptr(writefds),
            to_fdset_ptr(errorfds),
            to_ptr::<libc::timeval>(timeout) as *mut libc::timeval,
        )
    } {
        -1 => Err(io::Error::last_os_error()),
        res => Ok(res as usize),
    }
}

fn _make_timeval(duration: time::Duration) -> libc::timeval {
    libc::timeval {
        tv_sec: duration.as_secs() as i64,
        tv_usec: duration.subsec_micros() as i64,
    }
}

struct FdSet(libc::fd_set);

impl FdSet {
    fn new() -> FdSet {
        unsafe {
            let mut raw_fd_set = mem::MaybeUninit::<libc::fd_set>::uninit();
            libc::FD_ZERO(raw_fd_set.as_mut_ptr());
            FdSet(raw_fd_set.assume_init())
        }
    }
    fn _clear(&mut self, fd: RawFd) {
        unsafe { libc::FD_CLR(fd, &mut self.0) }
    }
    fn set(&mut self, fd: RawFd) {
        unsafe { libc::FD_SET(fd, &mut self.0) }
    }
    fn is_set(&mut self, fd: RawFd) -> bool {
        unsafe { libc::FD_ISSET(fd, &self.0) }
    }
}
