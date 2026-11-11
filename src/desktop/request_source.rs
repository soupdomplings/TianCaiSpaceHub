//! Request provenance includes managed execution as well as desktop clients.
//! These keys label observation only; they never authorize gateway requests.
use crate::desktop::clients::ClientId;

pub const LOCAL_ASSISTANCE_MCP: &str = "local-assistance-mcp";
pub const LOCAL_ASSISTANCE_MCP_NAME: &str = "协助处理 MCP";
pub const LOCAL_ASSISTANCE_MCP_RESOURCE: &str = "tiancai-local-assistance";
pub const GENIUS_BUDDY: &str = "genius-buddy";

pub fn keys() -> impl Iterator<Item = &'static str> {
    ClientId::ALL
        .into_iter()
        .map(ClientId::key)
        .chain(std::iter::once(LOCAL_ASSISTANCE_MCP))
        .chain(std::iter::once(GENIUS_BUDDY))
}

pub fn name(key: &str) -> Option<&'static str> {
    if key == GENIUS_BUDDY {
        Some("天才搭档")
    } else if key == LOCAL_ASSISTANCE_MCP {
        Some(LOCAL_ASSISTANCE_MCP_NAME)
    } else {
        ClientId::ALL
            .into_iter()
            .find(|client| client.key() == key)
            .map(ClientId::name)
    }
}
