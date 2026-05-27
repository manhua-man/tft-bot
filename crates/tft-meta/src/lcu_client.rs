//! Shared LCU HTTP client.
//!
//! Wraps reqwest::blocking::Client with LCU auth headers and self-signed cert support.

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use tft_executor::lcu_gate::Lockfile;

/// LCU REST client — wraps a blocking HTTP client with auth headers.
pub struct LcuClient {
    client: Client,
    base_url: String,
    auth: String,
}

impl LcuClient {
    /// Create a client from a parsed lockfile.
    pub fn from_lockfile(lf: &Lockfile) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(10))
            .no_proxy()
            .build()
            .context("building LCU HTTP client")?;

        Ok(Self {
            client,
            base_url: lf.base_url(),
            auth: lf.auth_header(),
        })
    }

    /// Send a GET request and parse JSON response.
    pub fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", &self.auth)
            .send()
            .with_context(|| format!("LCU GET {}", path))?;

        if !resp.status().is_success() {
            anyhow::bail!("LCU GET {} returned status {}", path, resp.status());
        }

        resp.json::<T>()
            .with_context(|| format!("parsing LCU GET {} response", path))
    }

    /// Send a POST request with optional JSON body.
    pub fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self
            .client
            .post(&url)
            .header("Authorization", &self.auth)
            .header("Content-Type", "application/json");

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().with_context(|| format!("LCU POST {}", path))?;

        if !resp.status().is_success() {
            anyhow::bail!("LCU POST {} returned status {}", path, resp.status());
        }

        resp.json::<T>()
            .with_context(|| format!("parsing LCU POST {} response", path))
    }

    /// Send a POST request expecting no JSON body (returns status).
    pub fn post_no_body(&self, path: &str) -> Result<u16> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", &self.auth)
            .header("Content-Type", "application/json")
            .send()
            .with_context(|| format!("LCU POST {}", path))?;

        Ok(resp.status().as_u16())
    }

    /// Send a DELETE request.
    pub fn delete(&self, path: &str) -> Result<u16> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .header("Authorization", &self.auth)
            .send()
            .with_context(|| format!("LCU DELETE {}", path))?;

        Ok(resp.status().as_u16())
    }
}
