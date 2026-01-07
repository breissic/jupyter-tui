use jupyter_protocol::{ExecuteRequest, JupyterMessage, JupyterMessageContent};
use serde_json::Value;
use std::fs;
use zmq::SocketType::{self, DEALER};

// TODO: Separate out this main function. Probably will want a lib.rs file eventually
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Spinning up manual kernel and hardcoding json filepath for now
    let contents = fs::read_to_string(
        "/home/carso/.local/share/jupyter/runtime/kernel-c7565892-0c0a-428e-8289-f8008083e080.json",
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

    // Send a msg
    shell_dealer.send("print('Hello, World!')", 0)?;

    Ok(())
}
