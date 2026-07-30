# API sharing and security

Managed Ollama and vLLM services remain bound to loopback. Lumen Source shares
selected models through its own OpenAI-compatible gateway at `/v1`, and the
gateway rejects every request that does not carry the current bearer token.
The token is generated or rotated in **Settings > Share an API**, is shown once
for copying, and is stored in the operating-system credential store.

**Allow other devices** is always an explicit choice. It never opens a firewall
port, creates a router rule, or claims to secure an external Ollama or vLLM
service. The page shows the address it expects clients to use and the exact
models exposed. Disabling sharing or revoking the token immediately stops the
gateway and restores loopback-only access.

The built-in gateway uses HTTP. For an untrusted LAN, VPN, or internet-facing
deployment, put a maintained TLS reverse proxy such as Caddy or nginx in front
of the displayed gateway address. Configure the proxy to:

1. Terminate TLS with a certificate clients trust.
2. Forward only `/v1/models`, `/v1/chat/completions`, and `/v1/embeddings`.
3. Preserve the `Authorization: Bearer ...` header.
4. Apply an IP allowlist, VPN policy, or equivalent network access control.
5. Keep the Lumen Source gateway unreachable from the public internet except
   through that proxy.

Do not expose a separately managed runtime through Lumen Source unless its
owner has independently configured authentication, TLS, firewall policy, and
updates. Lumen Source can connect to such a service but does not control or
secure it.
