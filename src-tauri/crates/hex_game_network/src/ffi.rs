use crate::NetworkManager;
use hex_game_core::engine;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::runtime::{Builder, Runtime};

static NETWORK_RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();
static NETWORK_MANAGERS: OnceLock<Mutex<HashMap<String, Arc<NetworkManager>>>> = OnceLock::new();

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

fn managers() -> &'static Mutex<HashMap<String, Arc<NetworkManager>>> {
    NETWORK_MANAGERS.get_or_init(|| Mutex::new(HashMap::new()))
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

    let mut registered_managers = match managers().lock() {
        Ok(managers) => managers,
        Err(_) => return make_c_string("E:Network session registry unavailable".to_string()),
    };
    if registered_managers.contains_key(&match_id) {
        return make_c_string(format!(
            "E:Match already has a native network session: {match_id}"
        ));
    }

    ensure_match_state(&match_id);
    if let Err(error) = engine::rebind_player_session(&match_id, "Player_1") {
        return make_c_string(format!("E:Could not claim host seat: {error}"));
    }
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            engine::release_player_session(&match_id, "Player_1");
            return make_c_string(format!("E:{error}"));
        }
    };
    let manager = Arc::new(NetworkManager::new(match_id.clone()));
    let node_id = match runtime.block_on(manager.host_match()) {
        Ok(node_id) => node_id,
        Err(error) => {
            engine::release_player_session(&match_id, "Player_1");
            return make_c_string(format!("E:{error}"));
        }
    };
    spawn_peer_reader(Arc::clone(&manager), match_id.clone(), true);
    registered_managers.insert(match_id, manager);
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

    let mut registered_managers = match managers().lock() {
        Ok(managers) => managers,
        Err(_) => {
            eprintln!("Native join failed: network session registry unavailable");
            return false;
        }
    };
    if registered_managers.contains_key(&match_id) {
        eprintln!("Native join failed: match already has a native network session: {match_id}");
        return false;
    }

    ensure_match_state(&match_id);
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Native join failed: {error}");
            return false;
        }
    };
    let manager = Arc::new(NetworkManager::new(match_id.clone()));
    if let Err(error) = runtime.block_on(manager.join_match(host_node_id)) {
        eprintln!("Native join failed: {error}");
        return false;
    }
    spawn_peer_reader(Arc::clone(&manager), match_id.clone(), false);
    registered_managers.insert(match_id, manager);
    true
}

/// Sends a local M:/A:/S/E action token to the connected peer.
///
/// # Safety
/// Both arguments must be null or point to valid, null-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_native_send_action_token(
    c_match_id: *const c_char,
    c_token: *const c_char,
) -> bool {
    if c_match_id.is_null() || c_token.is_null() {
        eprintln!("Native token send failed: null match ID or token");
        return false;
    }
    let match_id = match unsafe { CStr::from_ptr(c_match_id) }.to_str() {
        Ok(match_id) => match_id,
        Err(error) => {
            eprintln!("Native token send failed: invalid UTF-8 match ID: {error}");
            return false;
        }
    };
    let token = match unsafe { CStr::from_ptr(c_token) }.to_str() {
        Ok(token)
            if token.starts_with("M:") || token.starts_with("A:") || matches!(token, "S" | "E") =>
        {
            token
        }
        Ok(_) => {
            eprintln!("Native token send failed: unsupported referee action token");
            return false;
        }
        Err(error) => {
            eprintln!("Native token send failed: invalid UTF-8 token: {error}");
            return false;
        }
    };
    let manager = match managers().lock() {
        Ok(managers) => managers.get(match_id).cloned(),
        Err(_) => {
            eprintln!("Native token send failed: network manager registry unavailable");
            return false;
        }
    };
    let Some(manager) = manager else {
        eprintln!("Native token send failed: no network session for match {match_id}");
        return false;
    };
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Native token send failed: {error}");
            return false;
        }
    };
    match runtime.block_on(manager.send_action_token(token)) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("Native token send failed: {error}");
            false
        }
    }
}

fn ensure_match_state(match_id: &str) {
    if !engine::match_exists(match_id) {
        engine::store_match(
            match_id.to_string(),
            engine::generate_test_sandbox(match_id.to_string()),
        );
    }
}

fn spawn_peer_reader(manager: Arc<NetworkManager>, match_id: String, host: bool) {
    let task_runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Native peer reader could not start: {error}");
            return;
        }
    };
    task_runtime.spawn(async move {
        loop {
            if host {
                if let Err(error) = manager.accept_peer().await {
                    eprintln!("Native host failed to accept a peer: {error}");
                    break;
                }
            }
            loop {
                match manager.read_next_action_token().await {
                    Ok(token)
                        if token.starts_with("M:")
                            || token.starts_with("A:")
                            || matches!(token.as_str(), "S" | "E") =>
                    {
                        let acting_seat = if host { "P2" } else { "P1" };
                        if let Err(error) = engine::process_action(&match_id, acting_seat, &token) {
                            eprintln!("Native peer sent an invalid action: {error}");
                        }
                    }
                    Ok(_) => eprintln!("Native peer sent a non-action protocol message"),
                    Err(error) => {
                        let _ = manager.mark_disconnected().await;
                        if host {
                            break;
                        }
                        eprintln!("Native peer disconnected: {error}");
                        return;
                    }
                }
            }
        }
    });
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
