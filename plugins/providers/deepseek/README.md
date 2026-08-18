# DeepSeek Provider

A [Paloma](../../..) provider plugin serving DeepSeek models through the
official API. Advertises provider `DeepSeek` with a single backend,
`DeepSeek API` (API-key auth); lives in `plugins/providers/deepseek`.

## Installation

Download the newest `deepseek-plugin-v*` release from
[GitHub Releases](https://github.com/zluo01/Paloma/releases), then choose the
archive for your platform:

| Platform | Release asset |
|---|---|
| Linux x86-64 | `deepseek-provider-linux-amd64.tar.gz` |
| Linux ARM64 | `deepseek-provider-linux-arm64.tar.gz` |
| macOS Apple silicon | `deepseek-provider-macos-arm64.tar.gz` |
| Windows x86-64 | `deepseek-provider-windows-amd64.zip` |

Extract the archive to the location where you want to keep the plugin. The
plugin file is named `deepseek-provider` (`deepseek-provider.exe` on Windows).
The release also includes `SHA256SUMS` for verifying the download.

### Add the plugin to Paloma

1. Open **Settings → Plugins** and select **Add Provider Plugin…**.
2. Set **Command** to the extracted plugin file's absolute path.
3. Leave **Arguments** and **Environment** empty, then select **Add**.

Example **Command** values:

- Linux: `/home/<username>/paloma-plugins/deepseek-provider`
- macOS: `/Users/<username>/paloma-plugins/deepseek-provider`
- Windows: `C:\Users\<username>\paloma-plugins\deepseek-provider.exe`

Paloma starts the plugin automatically. Connect the **DeepSeek API** backend
and enter your DeepSeek API key when prompted.

### Portable JAR

The same release includes `deepseek-provider-VERSION.jar` for platforms
without a native build. It requires JDK 25. In **Add Provider Plugin…**, set:

- **Command**: `java`
- **Arguments**: `["-jar", "/absolute/path/to/deepseek-provider-VERSION.jar"]`

Leave **Environment** empty and select **Add**.

## Build from source

Requires JDK 25 and Maven.

```sh
git clone https://github.com/zluo01/Paloma.git
cd Paloma/plugins/providers/deepseek
mvn package
```

The build produces `target/deepseek-provider-fat.jar` — a single
self-contained JAR with every dependency shaded in. Register it using the
[Portable JAR](#portable-jar) instructions, substituting the built JAR's
absolute path.

## Models

The catalogue is static — `src/main/resources/models.json`. Whenever there is a new publication on new models, need to update the file.

## Development

```sh
mvn verify           # checkstyle + google-java-format gate, tests, fat jar
mvn test             # JUnit 5 unit tests (the section state machines)
mvn spotless:apply   # rewrite formatting in place
```
