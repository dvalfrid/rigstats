#![allow(unsafe_code)]
//! Who is at the other end of a sensor-service pipe.
//!
//! A pipe name can be taken by any local process when the service isn't
//! listening on it, and the app would then show (or act on) whatever that
//! process sends. The service runs in session 0, where no logged-in user's
//! process can; a pipe served from any other session is not the service.
//!
//! `#![allow(unsafe_code)]`: one Win32 call with no safe wrapper.

use std::os::windows::io::AsRawHandle;

/// The Windows session of the process serving `pipe`.
///
/// Asked of the pipe itself: looking the server process up by id
/// (`ProcessIdToSessionId`) is denied for a LocalSystem service.
pub fn server_session_id(pipe: &impl AsRawHandle) -> Option<u32> {
    let mut session = 0u32;
    // SAFETY: the handle is a live pipe handle borrowed from `pipe`; the
    // out-pointer is valid for the duration of the call.
    let ok = unsafe {
        winapi::um::winbase::GetNamedPipeServerSessionId(pipe.as_raw_handle().cast(), &mut session)
    };
    (ok != 0).then_some(session)
}

/// Whether `pipe` is served by a Windows service. Debug builds accept any
/// server: the development sidecar runs from a console, not as a service.
pub fn is_service_pipe(pipe: &impl AsRawHandle) -> bool {
    cfg!(debug_assertions) || server_session_id(pipe) == Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

    #[tokio::test]
    async fn reports_the_session_of_the_serving_process() {
        let name = format!(r"\\.\pipe\rigstats-test-session-{}", std::process::id());
        let server = ServerOptions::new().create(&name).unwrap();
        let client = ClientOptions::new().open(&name).unwrap();
        server.connect().await.unwrap();

        let mut own_session = 0u32;
        // SAFETY: the out-pointer is valid for the call.
        let ok = unsafe {
            winapi::um::processthreadsapi::ProcessIdToSessionId(
                std::process::id(),
                &mut own_session,
            )
        };
        assert_ne!(ok, 0);
        // The test itself is the server — a user session, never session 0.
        assert_eq!(server_session_id(&client), Some(own_session));
        assert_ne!(own_session, 0);
    }
}
