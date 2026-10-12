use std::error::Error;
use std::ffi::{CStr, CString};
use std::io::{self, Write};
use std::sync::Arc;

use hex_game_core::ffi::{
    hgs_free_string, hgs_get_current_state, hgs_initialize_test_match, hgs_process_token_for_match,
};
use hex_game_network::MatchSession;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

const MATCH_ID: &str = "p2p_sandbox_match";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("========================================");
    println!("          HEXGAMESIM P2P SANDBOX");
    println!("========================================");
    println!("1. Host a Match");
    println!("2. Join an Active Match");

    let mut input = BufReader::new(tokio::io::stdin());
    let role = prompt_line(&mut input, "Select role (1 or 2): ").await?;
    let session = match role.trim() {
        "1" => {
            let mut session = MatchSession::host_match().await?;
            println!("Host endpoint ready.");
            println!("Share this Node ID with the joining player:");
            println!("{}", session.local_node_id);
            println!("Waiting for a peer to connect...");
            session.accept_peer().await?;
            session
        }
        "2" => {
            let target_id = prompt_line(&mut input, "Paste the host Node ID: ").await?;
            MatchSession::join_match(target_id.trim()).await?
        }
        _ => return Err("Invalid selection. Choose 1 to host or 2 to join.".into()),
    };

    println!("Peer connected. Initializing local sandbox match...");
    initialize_test_match(MATCH_ID)?;
    println!("Local match initialized: {MATCH_ID}");

    let session = Arc::new(session);
    let incoming_session = Arc::clone(&session);
    tokio::spawn(async move {
        loop {
            match incoming_session.read_next_action_token().await {
                Ok(token) => {
                    println!("\nReceived action: {token}");
                    match process_token(MATCH_ID, &token) {
                        Ok(result) => println!("Action result: {result}"),
                        Err(error) => eprintln!("Could not apply incoming action: {error}"),
                    }
                    match get_current_state(MATCH_ID) {
                        Ok(state) => println!("Updated state: {state}"),
                        Err(error) => eprintln!("Could not read match state: {error}"),
                    }
                    print!("Move token (or /quit): ");
                    let _ = io::stdout().flush();
                }
                Err(error) => {
                    eprintln!("Incoming network stream ended: {error}");
                    break;
                }
            }
        }
    });

    print!("Move token (or /quit): ");
    io::stdout().flush()?;
    let mut line = String::new();
    while input.read_line(&mut line).await? != 0 {
        let token = line.trim();
        if token == "/quit" {
            break;
        }
        if token.is_empty() {
            print!("Move token (or /quit): ");
            io::stdout().flush()?;
            continue;
        }

        match process_token(MATCH_ID, token) {
            Ok(result) => {
                println!("Local action result: {result}");
                if result.starts_with("VALID_STEP:") {
                    if let Err(error) = session.send_action_token(token).await {
                        eprintln!("Could not broadcast action: {error}");
                    }
                }
            }
            Err(error) => eprintln!("Could not process action: {error}"),
        }
        print!("Move token (or /quit): ");
        io::stdout().flush()?;
        line.clear();
    }

    println!("Leaving P2P sandbox.");
    Ok(())
}

async fn prompt_line<R>(input: &mut R, prompt: &str) -> io::Result<String>
where
    R: AsyncBufRead + Unpin,
{
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    input.read_line(&mut line).await?;
    Ok(line)
}

fn initialize_test_match(match_id: &str) -> Result<(), Box<dyn Error>> {
    let match_id = CString::new(match_id)?;
    unsafe { hgs_initialize_test_match(match_id.as_ptr()) };
    Ok(())
}

fn process_token(match_id: &str, token: &str) -> Result<String, Box<dyn Error>> {
    let match_id = CString::new(match_id)?;
    let token = CString::new(token)?;
    let raw_result = unsafe { hgs_process_token_for_match(match_id.as_ptr(), token.as_ptr()) };
    if raw_result.is_null() {
        return Err("Core returned a null action result".into());
    }

    let result = unsafe { CStr::from_ptr(raw_result) }
        .to_string_lossy()
        .into_owned();
    unsafe { hgs_free_string(raw_result) };
    Ok(result)
}

fn get_current_state(match_id: &str) -> Result<String, Box<dyn Error>> {
    let match_id = CString::new(match_id)?;
    let raw_state = unsafe { hgs_get_current_state(match_id.as_ptr()) };
    if raw_state.is_null() {
        return Err("Core returned a null match state".into());
    }

    let state = unsafe { CStr::from_ptr(raw_state) }
        .to_string_lossy()
        .into_owned();
    unsafe { hgs_free_string(raw_state) };
    Ok(state)
}
