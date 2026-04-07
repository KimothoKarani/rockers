use std::env;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Ok};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::header::{self, HeaderMap};
use reqwest::{Client, ClientBuilder, Response};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::task::JoinSet;

const IMAGE: &str = "golang";
const REGISTRY_URL: &str = "https://registry-1.docker.io";
const AUTH_URL: &str = "https://auth.docker.io";
const SVC_URL: &str = "registry.docker.io";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = RegistryClient::new(IMAGE.to_string()).await?;
    let desc = client.get_platform_manifest_descriptor().await?;
    let ImageManifest { config, layers } = client.get_image_manifest(&desc).await?;

    client
        .download_blob(&config, format!("./tmp/layers/config.json"))
        .await?;

    let mut set = JoinSet::new();
    for (index, layer) in layers.into_iter().enumerate() {
        let client = client.clone();
        set.spawn(async move {
            let path = format!("./tmp/layers/{}_{}.tar.gz", index, layer.digest);
            client.download_blob_with_progress(&layer, path).await
        });
    }

    while let Some(res) = set.join_next().await {
        res.unwrap()?;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct RegistryClient {
    client: reqwest::Client,
    image: String,
    progress: Arc<MultiProgress>,
}

impl RegistryClient {
    async fn new(image: String) -> anyhow::Result<Self> {
        #[derive(Deserialize)]
        struct TokenResponse {
            token: String,
        }

        let token = Client::new()
            .get(format!("{AUTH_URL}/token",))
            .query(&[("service", SVC_URL)])
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
            image,
            client,
            progress: Arc::new(MultiProgress::new()),
        })
    }

    async fn get_manifests(&self, digest: &str) -> anyhow::Result<Response> {
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

    async fn get_manifest_list(&self) -> anyhow::Result<ManifestList> {
        let res = self
            .get_manifests("latest")
            .await?
            .json::<ManifestList>()
            .await?;

        Ok(res)
    }

    async fn get_platform_manifest_descriptor(&self) -> anyhow::Result<ManifestDescriptor> {
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

    async fn get_image_manifest(&self, desc: &ManifestDescriptor) -> anyhow::Result<ImageManifest> {
        let res = self
            .get_manifests(&desc.digest)
            .await?
            .json::<ImageManifest>()
            .await?;

        Ok(res)
    }

    async fn download_blob(
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

    async fn download_blob_with_progress(
        &self,
        layer: &Descriptor,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let bar = self.progress.add(ProgressBar::new(layer.size));
        bar.set_style(
            ProgressStyle::with_template(
            "{msg:<2} [{bar:40.green/white}] {bytes:>8}/{total_bytes:8} ({bytes_per_sec}, {eta})",
        )?
        .progress_chars("=>-")
        );

        let image = &self.image;
        let digest = &layer.digest;
        let digest_short = &digest[7..7 + 12];

        let url = format!("{REGISTRY_URL}/v2/library/{image}/blobs/{digest}");
        let mut resp = self.client.get(url).send().await?;

        bar.set_message(digest_short.to_owned());

        let mut file = tokio::fs::File::create(&path).await?;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk).await?;
            bar.inc(chunk.len() as u64);
        }
        file.flush().await?;

        bar.finish_with_message(format!("{digest_short}: Pull complete"));

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
