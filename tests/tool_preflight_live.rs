//! Opt-in check against an isolated local llama-server; never invokes tools.
use llamancer::{llama::client::Client, models::profile::Thinking};
#[test]
#[ignore = "requires an explicitly launched isolated fixture server"]
fn real_tool_preflight() {
    let endpoint = std::env::var("LLAMANCER_PROBE_ENDPOINT").unwrap();
    let client = Client::new(&endpoint).unwrap();
    let snapshot = client.snapshot().unwrap();
    assert!(!snapshot.router);
    let id = &snapshot.models[0].id;
    for mode in [Thinking::Off, Thinking::Medium, Thinking::High] {
        for responses in [false, true] {
            let result = client.tool_preflight(id, Some(mode), responses);
            println!(
                "{mode:?} {}: {result:?}",
                if responses {
                    "Responses SSE"
                } else {
                    "Chat Completions"
                }
            );
        }
    }
}
