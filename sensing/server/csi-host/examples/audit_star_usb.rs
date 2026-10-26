//! Decode an explicitly captured diagnostic UART journal using the shared Rust codec.
use serde::Deserialize;
use serde_json::json;
use std::{
    fs::File,
    io::{BufRead, BufReader},
};
#[derive(Deserialize)]
struct Chunk {
    mono_ns: u64,
    bytes: Vec<u8>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("UART chunk journal required")?;
    let mut deframer = csi_protocol::stream::Deframer::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let chunk: Chunk = serde_json::from_str(&line?)?;
        let mut input = chunk.bytes.as_slice();
        while let Some(Ok(raw)) = deframer.push(&mut input) {
            if let Ok(packet) = csi_protocol::star::Packet::decode(raw) {
                println!(
                    "{}",
                    json!({"mono_ns":chunk.mono_ns,"packet":csi_host::star::packet_json(&packet)})
                );
            } else if let Ok((header, csi_protocol::Message::Csi(csi))) =
                csi_protocol::frame::decode(raw)
            {
                println!(
                    "{}",
                    json!({"mono_ns":chunk.mono_ns,"csi":true,"rx":header.node.to_string(),"capture_seq":csi.capture_seq})
                );
            }
        }
    }
    Ok(())
}
