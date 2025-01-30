use reqwest::Identity;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Amount(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendCat {
    pub asset_id: String,
    pub address: String,
    pub amount: Amount,
    pub fee: Amount,
    #[serde(default)]
    pub memos: Vec<String>,
    #[serde(default)]
    pub auto_submit: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CoinDetail {
    pub coin_id: String,
    pub amount: u64,
    pub address: String,
    #[serde(rename = "type")]
    pub coin_type: Option<String>,
    pub asset_id: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub icon_url: Option<String>,
    pub outputs: Vec<Output>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub coin_id: String,
    pub amount: u64,
    pub address: String,
    pub receiving: bool,
    pub burning: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    pub fee: u64,
    pub inputs: Vec<CoinDetail>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CoinSpend {
    pub coin: DeserializableCoin,
    pub puzzle_reveal: String,
    pub solution: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeserializableCoin {
    pub parent_coin_info: String,
    pub puzzle_hash: String,
    pub amount: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendCatResponse {
    pub summary: Summary,
    pub coin_spends: Vec<CoinSpend>,
}

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Failed to load certificate")]
    CertificateError,
    #[error("Request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

pub struct SageClient {
    client: reqwest::Client,
    base_url: String,
}

impl SageClient {
    pub fn new(cert_path: &Path, key_path: &Path, base_url: String) -> Result<Self, ClientError> {
        let cert = std::fs::read(cert_path).map_err(|_| ClientError::CertificateError)?;
        let key = std::fs::read(key_path).map_err(|_| ClientError::CertificateError)?;

        let identity =
            Identity::from_pem(&[cert, key].concat()).map_err(|_| ClientError::CertificateError)?;

        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .identity(identity)
            .danger_accept_invalid_certs(true)
            .build()?;

        Ok(Self { client, base_url })
    }

    pub async fn send_cat(&self, request: SendCat) -> Result<SendCatResponse, ClientError> {
        let url = format!("{}/send_cat", self.base_url);
        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            return Err(ClientError::InvalidResponse(format!(
                "Status: {}, Body: {:?}",
                response.status(),
                response.text().await?
            )));
        }

        let response_body = response.json::<SendCatResponse>().await?;
        Ok(response_body)
    }

    pub async fn get_derivations(
        &self,
        request: GetDerivations,
    ) -> Result<GetDerivationsResponse, ClientError> {
        let url = format!("{}/get_derivations", self.base_url);
        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            return Err(ClientError::InvalidResponse(format!(
                "Status: {}, Body: {:?}",
                response.status(),
                response.text().await?
            )));
        }

        let response_body = response.json::<GetDerivationsResponse>().await?;
        Ok(response_body)
    }

    pub async fn send_xch(&self, request: SendXch) -> Result<SendCatResponse, ClientError> {
        let url = format!("{}/send_xch", self.base_url);
        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            return Err(ClientError::InvalidResponse(format!(
                "Status: {}, Body: {:?}",
                response.status(),
                response.text().await?
            )));
        }

        let response_body = response.json::<SendCatResponse>().await?;
        Ok(response_body)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GetDerivations {
    #[serde(default)]
    pub hardened: bool,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Derivation {
    pub index: u32,
    pub public_key: String,
    pub address: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetDerivationsResponse {
    pub derivations: Vec<Derivation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendXch {
    pub address: String,
    pub amount: Amount,
    pub fee: Amount,
    #[serde(default)]
    pub memos: Vec<String>,
    #[serde(default)]
    pub auto_submit: bool,
}
