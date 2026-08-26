//! The real HTTP client, behind the `network` feature.
//!
//! Blocking on purpose. Sync is a handful of requests triggered by a timer or
//! a user action, so an async runtime would be a large dependency and a large
//! attack surface bought for nothing.
//!
//! Deliberately thin: it converts types and nothing else. Every decision worth
//! testing — retry classification, upload framing, token refresh — lives in
//! [`crate::drive`], where it can be exercised without a network.

use std::collections::BTreeMap;

use crate::http::{HttpTransport, Request, Response};

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
            // A vault is small; a redirect chain is a sign something is wrong.
            .max_redirects(4)
            .user_agent(concat!("vardo/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl HttpTransport for UreqTransport {
    fn send(&mut self, request: Request) -> Result<Response, String> {
        // Built as an `http::Request` rather than through the agent's verb
        // helpers, whose builder type differs for with-body and without-body
        // requests and so cannot be assembled uniformly.
        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let outgoing = builder
            .body(request.body)
            .map_err(|e| format!("could not build request: {e}"))?;

        // ureq treats a non-2xx as an error; we want the response either way,
        // because the status and body are how failures get classified.
        let mut response = match self.agent.run(outgoing) {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(code)) => {
                return Ok(Response {
                    status: code,
                    headers: BTreeMap::new(),
                    body: Vec::new(),
                })
            }
            Err(e) => return Err(e.to_string()),
        };

        let status = response.status().as_u16();
        let headers: BTreeMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_string(), value.to_str().ok()?.to_string()))
            })
            .collect();
        let body = response
            .body_mut()
            .with_config()
            // A vault this large is not a vault. Bounded so a hostile or
            // broken endpoint cannot exhaust memory.
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
