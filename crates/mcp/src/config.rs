use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub meeting_agent_base_url: String,
    pub meeting_agent_api_key: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let host = env::var("MCP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("MCP_PORT")
            .unwrap_or_else(|_| "9080".to_string())
            .parse::<u16>()?;
        let meeting_agent_base_url = env::var("MEETING_AGENT_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:8080".to_string())
            .trim_end_matches('/')
            .to_string();
        let meeting_agent_api_key = env::var("MEETING_AGENT_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());

        Ok(Self {
            host,
            port,
            meeting_agent_base_url,
            meeting_agent_api_key,
        })
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
