use crate::MatchSession;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::runtime::{Builder, Runtime};

static NETWORK_RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();
static MATCH_SESSIONS: OnceLock<Mutex<HashMap<String, Arc<MatchSession>>>> = OnceLock::new();

fn runtime() -> Result<&'static Runtime, String> {
    match NETWORK_RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("Could not start Iroh runtime: {error}"))
    }) {
        Ok(runtime) => Ok(runtime),
        Err(error) => Err(error.clone()),
    }
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<MatchSession>>> {
    MATCH_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn make_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(error) => {
            eprintln!("Native networking FFI returned a string containing NUL: {error}");
            std::ptr::null_mut()
        }
    }
}

/// Starts and retains a host endpoint for this match, returning its Iroh Node ID.
/// The returned string must be released with `hgs_native_free_string`.
///
/// # Safety
/// `c_match_id` must be null or point to a valid, null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_native_host_match(c_match_id: *const c_char) -> *mut c_char {
    if c_match_id.is_null() {
        return make_c_string("E:INVALID_ARGUMENT".to_string());
    }

    let match_id = match unsafe { CStr::from_ptr(c_match_id) }.to_str() {
        Ok(match_id) if !match_id.is_empty() => match_id.to_owned(),
        Ok(_) => return make_c_string("E:INVALID_ARGUMENT".to_string()),
        Err(_) => return make_c_string("E:INVALID_UTF8".to_string()),
    };

    let mut registered_sessions = match sessions().lock() {
        Ok(sessions) => sessions,
        Err(_) => return make_c_string("E:Network session registry unavailable".to_string()),
    };
    if registered_sessions.contains_key(&match_id) {
        return make_c_string(format!("E:Match already has a native network session: {match_id}"));
    }

    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => return make_c_string(format!("E:{error}")),
    };
    let session = match runtime.block_on(MatchSession::host_match()) {
        Ok(session) => Arc::new(session),
        Err(error) => return make_c_string(format!("E:Could not host match: {error}")),
    };
    let node_id = session.local_node_id.clone();
    let accepting_session = Arc::clone(&session);
    runtime.spawn(async move {
        if let Err(error) = accepting_session.accept_peer().await {
            eprintln!("Native host peer acceptance failed: {error}");
        }
    });
    registered_sessions.insert(match_id, session);
    make_c_string(node_id)
}

/// Connects a retained local session to the supplied Iroh host Node ID.
///
/// # Safety
/// Both arguments must be null or point to valid, null-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_native_join_match(
    c_match_id: *const c_char,
    c_host_node_id: *const c_char,
) -> bool {
    if c_match_id.is_null() || c_host_node_id.is_null() {
        eprintln!("Native join failed: null match ID or host Node ID");
        return false;
    }

    let match_id = match unsafe { CStr::from_ptr(c_match_id) }.to_str() {
        Ok(match_id) if !match_id.is_empty() => match_id.to_owned(),
        Ok(_) => {
            eprintln!("Native join failed: empty match ID");
            return false;
        }
        Err(error) => {
            eprintln!("Native join failed: invalid UTF-8 match ID: {error}");
            return false;
        }
    };
    let host_node_id = match unsafe { CStr::from_ptr(c_host_node_id) }.to_str() {
        Ok(node_id) if !node_id.is_empty() => node_id,
        Ok(_) => {
            eprintln!("Native join failed: empty host Node ID");
            return false;
        }
        Err(error) => {
            eprintln!("Native join failed: invalid UTF-8 host Node ID: {error}");
            return false;
        }
    };

    let mut registered_sessions = match sessions().lock() {
        Ok(sessions) => sessions,
        Err(_) => {
            eprintln!("Native join failed: network session registry unavailable");
            return false;
        }
    };
    if registered_sessions.contains_key(&match_id) {
        eprintln!("Native join failed: match already has a native network session: {match_id}");
        return false;
    }

    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Native join failed: {error}");
            return false;
        }
    };
    let session = match runtime.block_on(MatchSession::join_match(host_node_id)) {
        Ok(session) => Arc::new(session),
        Err(error) => {
            eprintln!("Native join failed: {error}");
            return false;
        }
    };
    registered_sessions.insert(match_id, session);
    true
}

/// Frees a string allocated by `hgs_native_host_match`.
///
/// # Safety
/// `ptr` must be null or a pointer returned by `hgs_native_host_match`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_native_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = unsafe { CString::from_raw(ptr) };
    }
}
