use serde::Deserialize;

pub mod error;
pub mod response;
pub mod api;

pub struct HelixClient {
    http_client: reqwest::Client,
    client_id: String,
    client_secret: String,
}

impl HelixClient {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self { 
            http_client: reqwest::Client::new(), 
            client_id, 
            client_secret,
        }
    }
}