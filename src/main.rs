use jupyter_protocol::{ExecuteRequest, JupyterMessage, JupyterMessageContent};
use serde_json::{Value, json};
use std::fs;
use zmq::SocketType::{self};

// TODO: Separate out this main function. Probably will want a lib.rs file eventually
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Spinning up manual kernel and hardcoding json filepath for now
    let contents = fs::read_to_string(
        "/home/carso/.local/share/jupyter/runtime/kernel-90794aee-8192-4d1c-b78a-5f24e0138948.json",
    )
    .expect("File read failed");

    // Extract kernel communication info from json (move this to a function)
    let json: Value = serde_json::from_str(&contents)?;
    let ip = json["ip"].as_str().unwrap();
    let shell_port = json["shell_port"].as_u64().unwrap();

    // Set up zmq sockets
    let ctx = zmq::Context::new();
    let shell_dealer = ctx.socket(SocketType::DEALER)?;

    let endpoint = format!("tcp://{ip}:{shell_port}");
    shell_dealer.connect(&endpoint)?;

    // Set up subscriber to listen for kernel-published return messages
    let subscriber = ctx.socket(SocketType::SUB)?;
    let iopub_port = "35843";
    let sub_endpoint = format!("tcp://{ip}:{iopub_port}");
    subscriber.connect(&sub_endpoint)?;
    subscriber.set_subscribe(b"")?;

    // set up jupyter message
    let msg_header = json!({
        "msg_id": "test123",
        "session": "test_session",
        "username": "carso",
        "date": "2026-01-06",
        "msg_type": "execute_request",
        "version": "5.0"
    });

    let parent_header = json!({
        "msg_id": "test123"
    });

    let content = json!({
        "code": "print('Hello world!')",
        "silent": false,
        "store_history": true,
        "user_expressions": {}
    });

    let msg_header_str = msg_header.to_string();
    let parent_header_str = parent_header.to_string();
    let metadata_str = json!({}).to_string();
    let content_str = content.to_string();

    let frames: Vec<&[u8]> = vec![
        b"",                          // zmq identity
        b"<IDS|MSG>",                 // delimiter
        b"",                          // HMAC signature
        msg_header_str.as_bytes(),    // msg header
        parent_header_str.as_bytes(), // parent header
        metadata_str.as_bytes(),      // metadata
        content_str.as_bytes(),       // content
    ];

    // Send a msg
    shell_dealer.send_multipart(&frames, 0)?;

    // Receive the response from the kernel
    loop {
        let msg = subscriber.recv_string(0)?;
        println!("Received: {:?}", msg);
    }

    Ok(())
}
