# OCI Image Configuration: Walkthrough for Chang

This doc walks through a single pull of an image, step by step, showing the
actual JSON that comes back from the registry at each step, and the Rust
struct that JSON deserializes into. The goal is to trace one request all the
way through, so it's clear how config.rs connects to manifest.rs and
descriptor.rs.

## Important: rockers only pulls. We never push.

Everything below already exists on the registry before rockers does
anything. Someone ran docker build and docker push at some point in the
past. rockers' job is to read what's already there, not create it.

So when you see a JSON blob in this doc, think "this is what GET returns",
not "this is what we send".

## The example image

Let's say someone runs:

```
rockers pull registry.example.com/library/hello:latest
```

To keep the digests readable, I'm using shortened fake ones:

- sha256:AAAA... = digest of the manifest itself
- sha256:BBBB... = digest of the config blob
- sha256:CCCC... = digest of the layer, compressed (as stored in the manifest)
- sha256:DDDD... = digest of the layer, uncompressed (the diff_id)

These four digests are the thread we'll follow through every step.

## Step 1: fetch the manifest

rockers does:

```
GET /v2/library/hello/manifests/latest
```

The registry responds with this JSON (this is manifest.json):

```json
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.oci.image.manifest.v1+json",
  "config": {
    "mediaType": "application/vnd.oci.image.config.v1+json",
    "digest": "sha256:BBBB...",
    "size": 1469
  },
  "layers": [
    {
      "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
      "digest": "sha256:CCCC...",
      "size": 2818413
    }
  ]
}
```

This deserializes into Manifest, which Chang already wrote:

```rust
// manifest.rs
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u8,
    pub media_type: Option<MediaType>,
    pub artifact_type: Option<String>,
    pub config: Descriptor,
    pub layers: Vec<Descriptor>,
    pub subject: Option<Descriptor>,
    pub annotations: Option<HashMap<String, String>>,
}
```

Mapping JSON to struct, field by field:

| JSON field | Rust field | Value in our example |
|---|---|---|
| "schemaVersion": 2 | manifest.schema_version | 2 |
| "mediaType": "...manifest.v1+json" | manifest.media_type | Some(OCI_IMAGE_MANIFEST) |
| "config": { ... } | manifest.config | a Descriptor pointing at sha256:BBBB... |
| "layers": [ ... ] | manifest.layers | a Vec<Descriptor> with one entry, pointing at sha256:CCCC... |

At this point, rockers has two pointers and nothing else. config.rs
doesn't come into play yet. We have:

```
manifest.config  points at sha256:BBBB...  (we don't know what's there yet)
manifest.layers  points at sha256:CCCC...  (a gzip tarball)
```

## Step 2: look at manifest.config, which is a Descriptor

manifest.config is one value of this type, also Chang's code:

```rust
// descriptor.rs
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub media_type: MediaType,
    pub digest: Digest,
    pub size: u64,
    pub urls: Option<Vec<String>>,
    pub annotations: Option<HashMap<String, String>>,
    pub data: Option<String>,
    pub artifact_type: Option<String>,
    pub platform: Option<Platform>,
}
```

For our example, after parsing, manifest.config looks like:

```rust
Descriptor {
    media_type: MediaType::OCI_IMAGE_CONFIG,
    digest: Digest("sha256:BBBB..."),
    size: 1469,
    urls: None,
    annotations: None,
    data: None,
    artifact_type: None,
    platform: None,
}
```

The important part is digest: sha256:BBBB.... This is the address rockers
uses for the next fetch. Nothing has been downloaded yet except the small
manifest JSON from Step 1.

## Step 3: fetch the config blob using that digest

rockers does:

```
GET /v2/library/hello/blobs/sha256:BBBB...
```

The registry responds with this JSON. This is the blob that config.rs
exists to describe:

```json
{
  "architecture": "amd64",
  "os": "linux",
  "config": {
    "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
    "Entrypoint": ["/bin/hello"],
    "Cmd": ["--greeting", "hi"],
    "WorkingDir": "/home/alice",
    "User": "alice"
  },
  "rootfs": {
    "type": "layers",
    "diff_ids": ["sha256:DDDD..."]
  },
  "history": [
    {
      "created_by": "COPY hello /bin/hello"
    },
    {
      "created_by": "ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
      "empty_layer": true
    }
  ]
}
```

This is the JSON that ImageConfiguration deserializes. Now let's break that
struct down field by field against this exact JSON.

## Step 4: ImageConfiguration, field by field

```rust
// config.rs
#[serde(rename_all = "camelCase")]
pub struct ImageConfiguration {
    pub created: Option<String>,
    pub author: Option<String>,
    pub architecture: String,
    pub os: String,
    #[serde(rename = "os.version")]
    pub os_version: Option<String>,
    #[serde(rename = "os.features")]
    pub os_features: Option<Vec<String>>,
    pub variant: Option<String>,
    pub config: Option<Config>,
    pub rootfs: RootFs,
    pub history: Option<Vec<History>>,
}
```

Against our example JSON:

| Rust field | What it holds after parsing |
|---|---|
| architecture | "amd64" |
| os | "linux" |
| created, author, os_version, os_features, variant | all None, not present in our JSON |
| config | Some(Config { ... }), see Step 5 |
| rootfs | RootFs { fs_type: "layers", diff_ids: ["sha256:DDDD..."] } |
| history | Some(vec![History { ... }, History { ... }]), two entries |

The os.version and os.features renames don't come into play for this
particular image since it's Linux, but they exist for Windows images where
the JSON keys are literally "os.version" and "os.features", with a dot in
the key. rename_all = "camelCase" alone would have produced "osVersion",
which would not match, so those two fields need an explicit
#[serde(rename = "...")] each.

## Step 5: Config, the PascalCase one

```rust
// config.rs
#[serde(rename_all = "PascalCase")]
pub struct Config {
    pub user: Option<String>,
    pub exposed_ports: Option<HashMap<String, serde_json::Value>>,
    pub env: Option<Vec<String>>,
    pub entrypoint: Option<Vec<String>>,
    pub cmd: Option<Vec<String>>,
    pub volumes: Option<HashMap<String, serde_json::Value>>,
    pub working_dir: Option<String>,
    pub labels: Option<HashMap<String, String>>,
    pub stop_signal: Option<String>,
}
```

Against the "config": { ... } part of our JSON:

| JSON key | Rust field | Value |
|---|---|---|
| "Env" | env | Some(vec!["PATH=..."]) |
| "Entrypoint" | entrypoint | Some(vec!["/bin/hello"]) |
| "Cmd" | cmd | Some(vec!["--greeting", "hi"]) |
| "WorkingDir" | working_dir | Some("/home/alice") |
| "User" | user | Some("alice") |
| "ExposedPorts", "Volumes", "Labels", "StopSignal" | corresponding fields | all None, not present |

Notice the JSON keys are Env, Entrypoint, Cmd, capitalized. Every other
struct in config.rs and the rest of specs/ uses camelCase JSON keys
(mediaType, schemaVersion, diffIds would be camelCase if it existed).
Config is the one exception, hence rename_all = "PascalCase" instead of
rename_all = "camelCase". This is a Docker legacy thing carried into the
OCI spec for compatibility, not a mistake.

If rockers later execs this container with no overrides, the command run
would be:

```
/bin/hello --greeting hi
```

That's Entrypoint followed by Cmd, concatenated. If someone does
rockers run hello echo test, Cmd gets replaced by ["echo", "test"], but
Entrypoint stays, giving /bin/hello echo test.

## Step 6: RootFs and the two digest problem

```rust
// config.rs
pub struct RootFs {
    #[serde(rename = "type", deserialize_with = "deserialize_rootfs_type")]
    pub fs_type: String,
    pub diff_ids: Vec<String>,
}
```

Against our JSON: fs_type = "layers", diff_ids = ["sha256:DDDD..."].

Now here's the part that's genuinely confusing the first time, so let's use
our actual numbers.

We have two digests for the same layer:

- sha256:CCCC... came from manifest.layers[0].digest (Step 1)
- sha256:DDDD... came from config.rootfs.diff_ids[0] (this step)

Both refer to the same layer, but they are hashes of different bytes:

```
sha256:CCCC...  =  hash of the layer file AS STORED on the registry
                   (gzip compressed, this is what you download)

sha256:DDDD...  =  hash of the layer AFTER decompressing it
                   (this is what ends up on disk)
```

So the actual sequence rockers runs is:

```rust
// manifest.layers[0].digest is sha256:CCCC...
let compressed_bytes = fetch_blob("sha256:CCCC...")?;

// check 1: did we download the right bytes?
assert_eq!(sha256(&compressed_bytes), "CCCC...");

let uncompressed_bytes = gunzip(compressed_bytes)?;

// check 2: after decompressing, do we get what config.rs promised?
// config.rootfs.diff_ids[0] is sha256:DDDD...
assert_eq!(sha256(&uncompressed_bytes), "DDDD...");

unpack_tar(uncompressed_bytes)?;
```

manifest.rs gives us CCCC, the download address.
config.rs gives us DDDD, the "did unpacking give us the right thing" check.
Two different files, both legitimate, both needed.

## Step 7: History

```rust
// config.rs
pub struct History {
    pub created: Option<String>,
    pub author: Option<String>,
    pub created_by: Option<String>,
    pub comment: Option<String>,
    pub empty_layer: Option<bool>,
}
```

Our JSON has two history entries:

```json
{ "created_by": "COPY hello /bin/hello" }
```

```json
{ "created_by": "ENV PATH=...", "empty_layer": true }
```

The first one is the actual layer, it lines up with diff_ids[0]
(sha256:DDDD...). The second one is the ENV instruction, it produced no
filesystem change, hence empty_layer: true, and there is no corresponding
entry in diff_ids for it. This is why history can have more entries than
diff_ids. Not needed for pulling or running, just useful for showing build
history.

## Step 8: the Digest vs String issue

We already have a Digest type, used for Descriptor.digest in descriptor.rs:

```rust
// descriptor.rs
#[serde(try_from = "String", into = "String")]
pub struct Digest {
    algorithm: Algorithm,
    encoded: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Algorithm {
    #[default]
    Sha256,
    Sha512,
    Blake3,
    Unregistered(String),
}
```

I tried using Vec<Digest> for rootfs.diff_ids instead of Vec<String>, since
they're digests and Digest already validates the format. It did not compile:

```
the trait bound `Digest: serde::Serialize` is not satisfied
required for `Vec<Digest>` to implement `Serialize`
the trait `Serialize` is not implemented for `Digest`
```

The reason: #[serde(into = "String")] controls how the value converts, but
the derive macro still requires every field inside Digest to be Serialize.
algorithm: Algorithm is one of those fields, and Algorithm above does not
derive Serialize, notice the derive list only has Debug, Clone, Default,
PartialEq, Eq.

What I did instead: keep diff_ids: Vec<String> in config.rs, and parse each
entry into a Digest only when we actually verify a layer in Step 6, using
Digest::try_from(diff_id.clone())?.

Question for Chang: is it worth adding Serialize to Algorithm (or a
#[serde(bound(serialize = ""))] on Digest) so Vec<Digest> works directly?
Small change, but it's in descriptor.rs so wanted to raise it rather than
edit it myself.

## Summary of the whole chain

```
GET /manifests/latest
  -> Manifest { config: Descriptor(sha256:BBBB), layers: [Descriptor(sha256:CCCC)] }

GET /blobs/sha256:BBBB
  -> ImageConfiguration {
       architecture, os,
       config: Config { Entrypoint, Cmd, Env, User, ... },
       rootfs: RootFs { diff_ids: [sha256:DDDD] },
       history: [...]
     }

GET /blobs/sha256:CCCC
  -> compressed layer bytes
  -> verify against sha256:CCCC (from manifest)
  -> decompress
  -> verify against sha256:DDDD (from config.rootfs.diff_ids)
  -> unpack onto disk

Then, to run the container:
  use ImageConfiguration.config.entrypoint + cmd as the command
  use ImageConfiguration.config.env as environment variables
  use ImageConfiguration.config.user / working_dir for process setup
```

config.rs is the middle box in that chain, the thing that turns "a pointer
at sha256:BBBB" into "here's what to download and how to run it".