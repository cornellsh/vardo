//! The real HTTP client.
//!
//! Every status comes back as a response, never an error, because a `412` is
//! the answer a conditional write exists to get.

use std::collections::BTreeMap;

use crate::{Request, Response, Transport};

pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqTransport {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .http_status_as_error(false)
            // A redirect would resend the password to wherever it points.
            .max_redirects(0)
            .user_agent(concat!("vardo/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl Transport for UreqTransport {
    fn send(&mut self, request: Request) -> Result<Response, String> {
        let mut builder = ureq::http::Request::builder()
            .method(request.method)
            .uri(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let outgoing = builder
            .body(request.body)
            .map_err(|e| format!("could not build request: {e}"))?;
        let mut response = self.agent.run(outgoing).map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let headers: BTreeMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((
                    name.as_str().to_ascii_lowercase(),
                    value.to_str().ok()?.to_string(),
                ))
            })
            .collect();
        let body = response
            .body_mut()
            .with_config()
            .limit(512 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}
