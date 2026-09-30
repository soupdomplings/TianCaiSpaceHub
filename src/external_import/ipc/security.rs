//! Restrict import IPC to the current Windows user, including server identity.
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Foundation::{HANDLE, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::{
        Pipes::GetNamedPipeServerProcessId,
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

fn user_sid(process: HANDLE) -> io::Result<String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut length = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut length,
        );
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut raw = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut raw) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *raw.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(raw, length));
        LocalFree(raw.cast());
        Ok(sid)
    }
}

pub(super) fn current_user_sid() -> io::Result<String> {
    // GetCurrentProcess returns a pseudo-handle, which must not be closed.
    user_sid(unsafe { GetCurrentProcess() })
}

pub(super) fn create_server(first: bool, name: &str) -> io::Result<NamedPipeServer> {
    let sid = current_user_sid()?;
    let descriptor: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let mut raw = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            1,
            &mut raw,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: raw,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            );
        LocalFree(raw);
        result
    }
}

pub(super) fn verify_server_user(pipe: &NamedPipeClient) -> io::Result<()> {
    unsafe {
        let mut pid = 0;
        if GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let process = OwnedHandle::from_raw_handle(process);
        if user_sid(process.as_raw_handle())? != current_user_sid()? {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(())
    }
}
