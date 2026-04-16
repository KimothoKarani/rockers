use std::env;
use std::path::Path;

use anyhow::Context;
use indicatif::ProgressBar;
use reqwest::header::{self, HeaderMap};
use reqwest::{Client, ClientBuilder, Response};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

const AUTH_URL: &str = "https://auth.docker.io";
const REGISTRY_URL: &str = "https://registry-1.docker.io";

#[derive(Debug, Clone)]
pub struct RegistryClient {
    client: reqwest::Client,
    image: String,
    tag: String,
}

impl RegistryClient {
    pub async fn new(image: &str) -> anyhow::Result<Self> {
        let (image, tag) = image.split_once(":").unwrap_or((image, "latest"));

        #[derive(Deserialize)]
        struct TokenResponse {
            token: String,
        }

        let token = Client::new()
            .get(format!("{AUTH_URL}/token",))
            .query(&[("service", "registry.docker.io")])
            .query(&[("scope", format!("repository:library/{image}:pull"))])
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
            image: image.to_owned(),
            tag: tag.to_owned(),
        })
    }

    pub async fn get_manifests(&self, digest: &str) -> anyhow::Result<Response> {
        let image = &self.image;
        let url = format!("{REGISTRY_URL}/v2/library/{image}/manifests/{digest}");

        let res = self
            .client
            .get(url)
            .header(
                header::ACCEPT,
                "application/vnd.docker.distribution.manifest.list.v2+json",
            )
            .header(
                header::ACCEPT,
                "application/vnd.docker.distribution.manifest.v2+json",
            )
            .send()
            .await?;

        Ok(res)
    }

    pub async fn get_manifest_list(&self) -> anyhow::Result<ManifestList> {
        let res = self
            .get_manifests(&self.tag)
            .await?
            .json::<ManifestList>()
            .await?;

        Ok(res)
    }

    pub async fn get_platform_manifest_descriptor(&self) -> anyhow::Result<ManifestDescriptor> {
        let list = self.get_manifest_list().await?;

        let os = env::consts::OS;
        let arch = if env::consts::ARCH == "x86_64" {
            "amd64"
        } else {
            env::consts::ARCH
        };

        let desc = list
            .manifests
            .into_iter()
            .find(|m| m.platform.architecture == arch && m.platform.os == os)
            .with_context(|| {
                format!("No manifest found for the current platform (os: {os}, arch: {arch})")
            })?;

        Ok(desc)
    }

    pub async fn get_image_manifest(
        &self,
        desc: &ManifestDescriptor,
    ) -> anyhow::Result<ImageManifest> {
        let res = self
            .get_manifests(&desc.digest)
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
        let image = &self.image;
        let digest = &layer.digest;

        let url = format!("{REGISTRY_URL}/v2/library/{image}/blobs/{digest}");
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
        let image = &self.image;
        let digest = &layer.digest;

        let url = format!("{REGISTRY_URL}/v2/library/{image}/blobs/{digest}");
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageManifest {
    pub config: Descriptor,
    pub layers: Vec<Descriptor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub digest: String,
    pub size: u64,
}
