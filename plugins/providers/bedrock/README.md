# Amazon Bedrock Provider

A [Paloma](../../..) provider plugin serving foundation models on Amazon
Bedrock through the Converse API. Advertises provider `Amazon Bedrock` with a
single backend, `Bedrock API` (long-term API key or locally configured AWS
credentials); lives in `plugins/providers/bedrock`.

## Installation

Download a `bedrock-plugin-v*` release from
[GitHub Releases](https://github.com/zluo01/Paloma/releases), then choose the
archive for your platform:

| Platform | Release asset |
|---|---|
| Linux x86-64 | `bedrock-provider-linux-amd64.tar.gz` |
| Linux ARM64 | `bedrock-provider-linux-arm64.tar.gz` |
| macOS Apple silicon | `bedrock-provider-macos-arm64.tar.gz` |
| Windows x86-64 | `bedrock-provider-windows-amd64.zip` |

Extract the archive to the location where you want to keep the plugin. The
plugin file is named `bedrock-provider` (`bedrock-provider.exe` on Windows).
The release also includes `SHA256SUMS` for verifying the download.

### Add the plugin to Paloma

1. Open **Settings → Plugins** and select **Add Provider Plugin…**.
2. Set **Command** to the extracted plugin file's absolute path.
3. Leave **Arguments** and **Environment** empty, then select **Add**. If your
   AWS setup requires environment overrides, set them in **Environment**.

Example **Command** values:

- Linux: `/home/<username>/paloma-plugins/bedrock-provider`
- macOS: `/Users/<username>/paloma-plugins/bedrock-provider`
- Windows: `C:\Users\<username>\paloma-plugins\bedrock-provider.exe`

Paloma starts the plugin automatically.

## Connecting to Bedrock

Connect the **Bedrock API** backend and choose one of the authentication
methods below.

The plugin fetches the model catalogue when connecting. If that request
fails, the backend reports a connection error; check your authentication,
region, and access to Bedrock's model-listing APIs.

### Long-term API key

Enter your Bedrock long-term API key with its AWS region in
`<region>:<api-key>` format, for example:

```text
us-east-1:ABSK...
```

The connection prompt includes a link to AWS's instructions for generating a
long-term API key. The region prefix is required; do not enter the key alone.
An entered key takes precedence over locally configured AWS credentials.

### AWS credentials

Leave the connection input blank to use the AWS SDK's default credential
and region configuration on this device. Configure your AWS credentials and
region so they are available to the plugin process started by Paloma.

## Build from source

Requires Rust 1.98.1 or later and Cargo, matching the workspace requirement.

```sh
git clone https://github.com/zluo01/Paloma.git
cd Paloma
cargo build -p paloma-provider-bedrock --release --locked
```

The build produces `target/release/bedrock-provider`
(`target/release/bedrock-provider.exe` on Windows). Register it using the
[Add the plugin to Paloma](#add-the-plugin-to-paloma) instructions,
substituting the built binary's absolute path.

## Models

The catalogue is fetched from Bedrock rather than a static model list. The
plugin selects text-output models with streaming support, excludes models
marked inactive and known unsupported models, and uses system-defined
inference profiles for models without direct on-demand inference. Regional
profiles are preferred over global profiles when both are found.

Model filtering and Claude reasoning capabilities are maintained in
`src/runtime/models.rs`. The reasoning capability mappings and unsupported
model exclusions are hard-coded and need updating when model support changes.

## Development

Run from the repository root:

```sh
cargo +nightly fmt -p paloma-provider-bedrock -- --check
cargo clippy -p paloma-provider-bedrock --all-targets --locked -- -D warnings
cargo test -p paloma-provider-bedrock --locked
```

The formatting check requires nightly Rust with the `rustfmt` component.
