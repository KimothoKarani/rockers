use std::env;
use std::path::Path;

use anyhow::Context;
use indicatif::ProgressBar;
use reqwest::header::{self, HeaderMap};
use reqwest::{Client, ClientBuilder};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

const AUTH_URL: &str = "https://auth.docker.io";
const REGISTRY_URL: &str = "https://registry-1.docker.io";

#[derive(Debug, Clone)]
pub struct RegistryClient {
    client: reqwest::Client,
    repository: String,
    tag: String,
}

impl RegistryClient {
    pub async fn new(image: &str) -> anyhow::Result<Self> {
        let (image, tag) = image.split_once(":").unwrap_or((image, "latest"));
        let repository = if image.contains('/') {
            image.to_owned()
        } else {
            format!("library/{image}")
        };

        #[derive(Deserialize)]
        struct TokenResponse {
            token: String,
        }

        let token = Client::new()
            .get(format!("{AUTH_URL}/token",))
            .query(&[("service", "registry.docker.io")])
            .query(&[("scope", format!("repository:{repository}:pull"))])
            .send()
            .await?
            .json::<TokenResponse>()
            .await?
            .token;

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", token).parse().unwrap(),
        );
        let client = ClientBuilder::new().default_headers(headers).build()?;

        Ok(Self {
            client,
            repository,
            tag: tag.to_owned(),
        })
    }

    pub async fn get_manifest_response(
        &self,
        reference: &str,
        accept: &str,
    ) -> anyhow::Result<reqwest::Response> {
        let repository = &self.repository;
        let url = format!("{REGISTRY_URL}/v2/{repository}/manifests/{reference}");

        let res = self
            .client
            .get(url)
            .header(header::ACCEPT, accept)
            .send()
            .await?
            .error_for_status()?;

        Ok(res)
    }

    pub async fn get_manifest_list(&self) -> anyhow::Result<ManifestList> {
        let accept = "application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.index.v1+json";

        let res = self
            .get_manifest_response(&self.tag, accept)
            .await?
            .json::<ManifestList>()
            .await?;

        Ok(res)
    }

    pub async fn get_platform_manifest_descriptor(&self) -> anyhow::Result<ManifestDescriptor> {
        let os = env::consts::OS;
        let arch = if env::consts::ARCH == "x86_64" {
            "amd64"
        } else {
            env::consts::ARCH
        };

        if let Ok(list) = self.get_manifest_list().await {
            let desc = list
                .manifests
                .into_iter()
                .find(|m| m.platform.architecture == arch && m.platform.os == os)
                .with_context(|| {
                    format!("No manifest found for the current platform (os: {os}, arch: {arch})")
                })?;

            return Ok(desc);
        }

        let manifest = self.get_image_manifest(&self.tag).await?;
        Ok(ManifestDescriptor {
            digest: self.tag.clone(),
            platform: Platform {
                architecture: arch.to_owned(),
                os: os.to_owned(),
            },
            size: manifest.config.size,
        })
    }

    pub async fn get_image_manifest(&self, reference: &str) -> anyhow::Result<ImageManifest> {
        let accept = "application/vnd.docker.distribution.manifest.v2+json, application/vnd.oci.image.manifest.v1+json";

        let res = self
            .get_manifest_response(reference, accept)
            .await?
            .json::<ImageManifest>()
            .await?;

        Ok(res)
    }

    pub async fn download_blob(
        &self,
        layer: &Descriptor,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let repository = &self.repository;
        let digest = &layer.digest;

        let url = format!("{REGISTRY_URL}/v2/{repository}/blobs/{digest}");
        let resp = self.client.get(url).send().await?;
        let bytes = resp.bytes().await?;
        tokio::fs::write(path, bytes).await?;

        Ok(())
    }

    pub async fn download_blob_with_progress(
        &self,
        layer: &Descriptor,
        path: impl AsRef<Path>,
        bar: ProgressBar,
    ) -> anyhow::Result<()> {
        let repository = &self.repository;
        let digest = &layer.digest;

        let url = format!("{REGISTRY_URL}/v2/{repository}/blobs/{digest}");
        let mut resp = self.client.get(url).send().await?;

        let mut file = tokio::fs::File::create(&path).await?;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk).await?;
            bar.inc(chunk.len() as u64);
        }
        file.flush().await?;

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestList {
    pub manifests: Vec<ManifestDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestDescriptor {
    pub digest: String,
    pub platform: Platform,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    pub architecture: String,
    pub os: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageManifest {
    pub config: Descriptor,
    pub layers: Vec<Descriptor>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub digest: String,
    pub size: u64,
}
