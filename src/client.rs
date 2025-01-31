use reqwest::Identity;
use sage_api::{
    GetDerivations, GetDerivationsResponse, SendCat, SendCatResponse, SendXch, SignCoinSpends,
    SignCoinSpendsResponse,
};
use std::path::Path;
use thiserror::Error;

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

    pub async fn sign_coin_spends(
        &self,
        request: SignCoinSpends,
    ) -> Result<SignCoinSpendsResponse, ClientError> {
        let url = format!("{}/sign_coin_spends", self.base_url);

        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            return Err(ClientError::InvalidResponse(format!(
                "Status: {}, Body: {:?}",
                response.status(),
                response.text().await?
            )));
        }

        let response_body = response.json::<SignCoinSpendsResponse>().await?;
        Ok(response_body)
    }
}
