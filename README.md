<h1 align="center">Paloma</h1>

<p align="center">
  Need something done? Open Paloma and ask.
</p>

Paloma is an extensible desktop launcher that puts search, actions, and AI
behind a single global shortcut. Search directly for an app or file, or
describe what you need in your own words. Paloma can use its tools to carry the
task through—not just tell you what to do—and extensions expand what it can
find and do.

> [!IMPORTANT]
> Paloma is under active development. Expect changes to the interface, plugin
> protocols, and stored data before the first stable release.

<details>
<summary><strong>Preview Paloma on macOS</strong></summary>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/macos/dark/paloma-preview.gif">
    <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/macos/light/paloma-preview.gif">
    <img src="docs/screenshots/macos/light/paloma-preview.gif" width="760" alt="Animated preview of Paloma's launcher, search, tool permissions, chat, session history, plugins, and services">
  </picture>
</p>

</details>

## Highlights

- **Work in parallel.** Run multiple AI tasks at the same time without one
  blocking another.
- **Use the models you want.** Change models whenever you like, including in
  the middle of a conversation.
- **Choose your AI service.** OpenAI and Anthropic are included, and provider
  plugins can add other services.
- **Expand what Paloma can do.** Add new search sources and actions through
  extensions, or connect tools from MCP servers. Enable only what you want to
  use.
- **Action with your consent.** Paloma pauses before uncertain actions run on
  your system. You decide what to allow once, what to remember, and when a chat
  can continue without asking.

## Platform support

| Platform | Availability | Requirements |
| --- | --- | --- |
| Linux | Build from source | A compatible Wayland desktop is required. See the Linux installation requirements below. |
| macOS | DMG | Apple silicon and macOS 26.5 or later. The first launch requires one-time approval in macOS settings. |
| Windows | ZIP | Windows 10 version 1809 or later on an x64 PC. |

## Install

### Linux

Linux support is currently tested on Fedora with KDE Plasma.

The Linux application requires:

- Rust 1.94 or later;
- GTK 4.22 or later, libadwaita 1.9 or later, and GTK4 Layer Shell;
- a Wayland compositor with Layer Shell support;
- an XDG desktop portal backend with Global Shortcuts support; and
- `wl-clipboard` for clipboard history (optional).

Install a release build for the current user:

```sh
./scripts/install-linux.sh
```

The script installs Paloma to `~/.local/bin` and adds its desktop entry. To
remove both:

```sh
./scripts/install-linux.sh --uninstall
```

On Linux, your desktop may ask you to approve the global shortcut the first
time Paloma starts.

### macOS

Download the latest DMG from [GitHub Releases](../../releases). Current builds
are ad-hoc signed, so macOS requires you to approve Paloma in
**System Settings → Privacy & Security** the first time you open it.

### Windows

Download the latest `Paloma-<version>-x64.zip` from
[GitHub Releases](../../releases). Extract all files to a folder, then launch
Paloma through `Paloma.exe`. Keep all extracted files in the same folder.

## Plugins

Paloma provides these optional plugins as separate downloads:

| Plugin | Type | What it adds | Installation |
| --- | --- | --- | --- |
| DeepSeek | Provider | Adds DeepSeek models through the official API. A DeepSeek API key is required. | [Install DeepSeek](plugins/providers/deepseek/README.md#installation) |
| WebFetch | Extension | Lets models read public web pages as markdown, plain text, or raw HTML. Useful for models without built-in web search, such as DeepSeek. | [Install WebFetch](plugins/extensions/webfetch/README.md#installation) |

Installed plugins are managed under **Settings → Plugins**.

## Extend Paloma

Paloma plugins can be written in any language using the provided protobuf
schemas:

- **Extension plugins** add new search capabilities and tools. Start with the
  [extension schema](schema/extension) and [code example](plugins/extensions/webfetch).
- **Provider plugins** add new LLM providers. Start with the
  [provider schema](schema/provider) and [code example](plugins/providers/deepseek).

See the [core documentation](core/README.md) for the architecture, protocols,
authentication flows, and permission workflow.

## Development

### Linux

Development is currently tested on Fedora with KDE Plasma. Install the build
dependencies:

```sh
sudo dnf install gcc pkgconf-pkg-config \
  gtk4-devel libadwaita-devel gtk4-layer-shell-devel
```

Rust 1.94 or later is required. Build the default workspace and start the
Linux application:

```sh
cargo build
cargo run
```

### macOS

Rust 1.94 or later and Xcode are required. Generate the Swift bindings, then
open the project:

```sh
scripts/build-swift-bindings.sh --debug
open gui/macos/Paloma/Paloma.xcodeproj
```

Build and run the `Paloma` scheme in Xcode. Subsequent builds refresh the Rust
library and Swift bindings automatically.

### Windows

Windows development requires:

- Windows 10 version 1809 (build 17763) or later;
- Rust 1.94 or later;
- the .NET 10 SDK; and
- Microsoft C++ Build Tools, including the **Desktop development with C++**
  workload.

Build and start Paloma from the repository root:

```powershell
dotnet run --project gui/windows/Paloma/Paloma.csproj
```

Run the Windows frontend tests with:

```powershell
dotnet test gui/windows/Paloma.Tests/Paloma.Tests.csproj
```

To create the Windows release ZIP:

```powershell
./scripts/release-windows.ps1
```

The script builds for the current machine's architecture and writes
`target/windows/Paloma-<version>-<arch>.zip`.

### Checks

```sh
cargo +nightly fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets
```

### Project structure

| Path | Contents |
| --- | --- |
| [`core/`](core) | Shared runtime, persistence, plugin host, and UniFFI API |
| [`gui/linux/`](gui/linux) | GTK4 and libadwaita frontend |
| [`gui/macos/`](gui/macos) | SwiftUI frontend and Xcode project |
| [`gui/windows/`](gui/windows) | WinUI 3 frontend, C# tests and protobuf bindings, and Rust gRPC core process |
| [`plugins/extensions/`](plugins/extensions) | Bundled and example extensions |
| [`plugins/providers/`](plugins/providers) | Bundled and example model providers |
| [`schema/`](schema) | Protobuf contracts for plugins |

## Support

Before reporting a bug, search the [existing issues](../../issues). If it has
not already been reported, open a new issue using the bug report template.

Logs are written to:

- Linux: `${XDG_STATE_HOME:-~/.local/state}/paloma/logs/`
- macOS: `~/Library/Logs/Paloma/`
- Windows: `%LOCALAPPDATA%\Paloma\logs\`

## Contributing

Paloma is still taking shape, so development currently follows the project's
planned work. Accepting ad hoc features at this stage would make it harder to
keep the product focused, so feature requests are not being accepted for now unless they are related to any of the epic topic.

If you would like to contribute, choose an issue marked as available, comment
that you would like to work on it, and wait for it to be assigned before you
start. This helps avoid duplicated or wasted work. Pull requests that are not
tied to an assigned issue will be closed.

Please make sure every change is well tested.

### LLM-assisted contributions

I do not reject contributions simply because an LLM was used to help write
them. An LLM is a tool; the contributor must still own the change and the
reasoning behind it. Before submitting, you should be able to explain the
code, understand its behavior and tradeoffs, and verify it with appropriate
tests. LLM-generated work submitted without that level of human ownership
will be rejected.
