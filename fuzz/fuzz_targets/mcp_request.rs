#![no_main]

use libfuzzer_sys::fuzz_target;
use rmcp::model::ClientJsonRpcMessage;

fuzz_target!(|data: &[u8]| {
    if data.len() > 16 * 1024 {
        return;
    }
    // Both Qiyas MCP transports use rmcp's wire parser. No server is started here.
    if let Ok(request) = serde_json::from_slice::<ClientJsonRpcMessage>(data) {
        let bytes = serde_json::to_vec(&request).expect("serializable MCP message");
        let _ = serde_json::from_slice::<ClientJsonRpcMessage>(&bytes);
    }
});
