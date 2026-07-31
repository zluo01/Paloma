# Paloma Core

## Development

```sh
cargo test -p paloma-core     # unit + storage tests
cargo clippy -p paloma-core   # lint gate; clone lints are workspace warnings
```

## Architecture

### High Level Search Workflow:

```mermaid
sequenceDiagram
    participant FE as frontend overlay
    participant EC as ExtensionController
    participant DB as SQLite
    participant EP as extension plugins (stdio)
    FE ->> EC: search(input)
    EC ->> DB: read disabled plugins · capabilities
    par one JoinSet task per enabled search capability
        EC ->> EP: search(capability, input)
        EP --) FE: SearchRenderEvent::Append — unordered, as each finishes
    end
    EC --) FE: Done — after the last capability
    Note over FE, EP: activating a result
    FE ->> EC: run_search_action(id, action)
    EC ->> EP: action
    EP -->> FE: Behavior (what the overlay does next)
```

### High Level Chat Workflow:

```mermaid
flowchart TD
    FE["Frontend"]
    START["Start or resume chat"]
    DB[("SQLite")]

    subgraph LOOP["Turn loop"]
        direction TB
        MODEL["Call the model through a provider"]
        STREAM["Stream response events"]
        HAS_TOOLS{"Tool calls?"}
        PERMISSION["Resolve permissions"]
        RUN["Run allowed extension and MCP tools in parallel"]
        RESULTS["Add tool results to the conversation"]
        DONE["Turn complete"]

        MODEL --> STREAM
        STREAM --> HAS_TOOLS
        HAS_TOOLS -->|no| DONE
        HAS_TOOLS -->|yes| PERMISSION
        PERMISSION -->|allowed| RUN
        PERMISSION -->|denied| RESULTS
        RUN --> RESULTS
        RESULTS -->|next model step| MODEL
    end

    FE -->|prompt| START
    START --> MODEL
    STREAM -->|render| FE
    STREAM -->|persist| DB
    RESULTS -->|persist| DB
    FE -.->|when prompted| PERMISSION
    DB -.->|restore session| START
```

## Plugins

Core does not include any domain specific logic. Every model, search source,
and tool the app offers is served by a plugin. Plugins run outside the core
process, as child processes speaking varint-delimited protobuf
over stdio.

### Extensions

Extensions add functionality to both surfaces of the app: search and LLM
chat. An extension bundles one or more capabilities, and each capability can
act as a search source, as a tool available to the model, or as both. Every
extension speaks the same wire protocol, defined in
[`schema/extension`](../schema/extension). To write your own, see
[`webfetch`](../plugins/extensions/webfetch) for a complete reference
implementation.

#### Extension Call Flow

```mermaid
sequenceDiagram
    participant C as core (ExtensionController)
    participant EP as extension plugin
    Note over C, EP: startup — core spawns the process, stdio protobuf
    C ->> EP: HandshakeRequest
    EP -->> C: HandshakeResponse — extension_id · capabilities (search / tool facets) · tool specs
    Note over C, EP: a tool call (search: see Architecture)
    C ->> EP: InvokeToolRequest (session_id, call_id, args)
    EP -->> C: InvokeToolResponse — ToolContent or Binary
    opt session cancelled
        C ->> EP: CancelToolRequest (session_id)
        EP -->> C: CancelToolResponse
    end
```

### Providers

Providers connect the app to LLM services. A provider bundles one or more
backends, each declared in the handshake with its unique id, description,
icon, and required auth method. Every provider speaks the same wire
protocol, defined in
[`schema/provider`](../schema/provider). Anthropic and OpenAI ship as
built-ins, served through the same re-exec mechanism as extensions; to
write your own, see [`deepseek`](../plugins/providers/deepseek) for a
complete reference implementation. Connecting a backend to an account is
covered in [Authentication → Providers](#providers-1).

#### Provider Call Flow

```mermaid
sequenceDiagram
    participant C as core (ProviderController)
    participant PP as provider plugin
    participant API as model API
    Note over C, PP: startup — core spawns the process, stdio protobuf
    C ->> PP: HandshakeRequest
    PP -->> C: HandshakeResponse — provider_id · backends (id, auth kind, icon)
    C ->> PP: InitializeBackendsRequest — stored auths replayed
    PP ->> API: validate credentials
    PP -->> C: InitializeBackendsResponse (health via BackendHealthStatus / BackendInitError)
    C ->> PP: ListModelsRequest
    PP -->> C: ListModelsResponse — models + reasoning efforts
    Note over C, API: one chat step
    C ->> PP: ChatRequest (session_id, model, effort, messages, tools)
    PP ->> API: streaming request (HTTPS)
    loop until the stream ends
        PP --) C: ChatResponse — TextDelta · ReasoningDelta · OutputItem
    end
    PP --) C: ChatResponse — Done (or Error)
    opt session cancelled
        C ->> PP: CancelChatRequest (session_id)
        PP -->> C: CancelChatResponse
    end
    Note over C, API: OAuth token refresh — the one plugin-initiated message
    PP ->> API: refresh the expired token
    PP --) C: AuthUpdateRequest — core persists the new refresh_token
```

### MCPs

MCP servers internally are handled through the [`rmcp`](https://crates.io/crates/rmcp)
crate. MCPs will join the same catalogue and
permission gate as extension tools. Connecting a remote server
is covered in [Authentication → MCPs](#mcps-1).

## Authentication

Provider backends and protected remote MCP servers may require authentication.
Successful connections are saved and restored on future launches.

### Providers

Each backend supports one sign-in method: API key, device code, or browser
OAuth. The app presents the matching flow, and the provider plugin handles
authentication with the service. OAuth credentials refresh automatically.

#### Provider Auth Flow

```mermaid
sequenceDiagram
    participant FE as frontend
    participant C as core (ProviderController)
    participant PP as provider plugin
    participant SVC as auth service
    FE ->> C: init_connection(backend)
    C ->> PP: InitConnectionRequest
    alt manual API key
        PP -->> C: ConnectionPayload — ManualInput (instructions_url)
        C -->> FE: key-entry dialog
        Note over FE, SVC: user creates a key in the service console
        FE ->> C: finalize_connection(ApiKey, key)
        C ->> PP: FinalizeConnectionRequest — api_key
    else device code
        PP ->> SVC: request a device authorization
        PP -->> C: ConnectionPayload — DeviceCode (verification_url · user_code · transaction_payload)
        C -->> FE: challenge dialog — show user_code, open verification_url
        Note over FE, SVC: user enters the code with the service
        FE ->> C: finalize_connection(DeviceCode, transaction_payload)
        C ->> PP: FinalizeConnectionRequest — transaction_payload
        PP ->> SVC: redeem the device authorization
    else browser OAuth
        PP -->> C: ConnectionPayload — BrowserRedirect (authorization_url)
        C -->> FE: open authorization_url · paste-back dialog
        Note over FE, SVC: user authorizes — the service displays a response to paste
        FE ->> C: finalize_connection(BrowserOauth, authorization_response)
        C ->> PP: FinalizeConnectionRequest — authorization_response
        PP ->> SVC: exchange the code for tokens (PKCE)
    end
    PP -->> C: FinalizeConnectionResponse — ProviderAuth
    C ->> PP: InitBackendRequest (auth)
    PP -->> C: backend ready
```

### MCPs

> [!NOTE]
> OAuth requires the server to support Dynamic Client Registration. Servers
> that require pre-registered client credentials are not supported by `rmcp`.

Local servers and remote servers without authentication connect directly.
Protected remote servers use browser OAuth: the app opens the authorization
page and completes the connection after approval. OAuth credentials refresh
automatically.

#### MCP Auth Flow

```mermaid
sequenceDiagram
    participant FE as frontend
    participant C as core (McpController)
    participant B as browser
    participant MS as MCP server
    FE ->> C: init_mcp_connection(config)
    C ->> MS: OAuth discovery · client registration
    C -->> FE: authorization URL
    FE ->> B: open the URL
    Note over B, MS: user authorizes with the server
    FE ->> C: finalize_mcp_connection(config)
    B -->> C: authorization redirect
    C ->> MS: exchange the code for tokens
    C ->> MS: connect · list tools
```

## Permissions

Every extension and MCP tool call passes through the permission gate before it
runs. Shell calls are classified from their command line; other tools are
classified by plugin and tool name. Classification begins as soon as the call
appears in the model's response, so safe calls and calls covered by an existing
approval can proceed without prompting when execution starts.

Shell commands are parsed conservatively. Read-only commands may be allowed
automatically, while unsupported commands are denied. Pipelines and command
lists are judged by their strictest command; risky or ambiguous input always
requires an explicit decision and cannot create a persistent approval.

Pending calls wait for the user to allow or deny them. An ordinary command can
be allowed once or approved for future matching calls. Composite shell commands
can instead be approved for the current session. Saved approvals persist across
launches and can be removed from settings. A denied, invalid, or cancelled call
is not executed, and the outcome is returned to the model.

#### Permission Decision Flow

```mermaid
sequenceDiagram
    participant T as turn (tool call)
    participant PWM as PermissionWorkflowManager
    participant PC as PermissionController
    participant DB as SQLite
    participant FE as frontend

    Note over T, PC: a tool call appears in the stream — shell gates on its argv, every other tool on its plugin · tool name
    T ->> PWM: init_permission_workflow(session_id, call_id, command)
    PWM ->> PC: classify(command)
    Note over PC: tree-sitter shell parse · strip transparent wrappers
    PC ->> PC: safety_check per atom — safe-read allowlist vs dangerous shapes
    opt verdict is Unknown
        PC ->> DB: stored prefix rules — exact or glob match
    end
    PC -->> PWM: Allow · Unknown · AskNoPersist · NotExecutable (a composite folds to its strictest atom)
    Note over PWM: pre-resolve — Allow and session-allowlisted composites run promptless · NotExecutable becomes Deny · the rest stays pending
    PWM -->> FE: decision options with the ToolCall render event (empty when pre-resolved)
    T ->> PWM: authorize(call_id) — execution blocks on the tracker
    alt pre-resolved
        PWM -->> T: Allow / Deny immediately
    else pending — the user decides
        FE ->> PWM: decide — AllowOnce · Allow prefix ± glob · AllowSession · IgnorePermission · Deny
        opt persistent choices
            PWM ->> DB: Allow saves a prefix rule — session choices stay in memory
        end
        PWM -->> T: resolved Allow / Deny
    end
    Note over T: Allow runs the tool — Deny · Error · cancellation return the refusal to the model as the tool result
```
