# Build provenance

The headless sidecar binaries in `service/` were built from the
user-supplied local Dify2API project. No runtime configuration, API key,
log, GUI asset, or pre-existing archive was copied into this plugin.

Build profile:

```text
Go module: dify2api
Go directive: 1.26.5
Build tags: headless
CGO: disabled
Flags: -mod=readonly -buildvcs=false -trimpath -ldflags="-s -w -buildid="
Upstream TLS: certificate and hostname verification disabled for private Dify deployments
```

Artifacts:

```text
windows/amd64  dify2api-server.exe  E92B2C99A2AE0FB250C1C47A9E2157763ABAF5939888F6FFCD18B5FDB79CEA94
linux/amd64    dify2api-server      E98B7EF47991ED82FC74FE413896511A53366FBDB5E02C305FC3D4666F5C994C
```

The supplied project did not contain a project-level `LICENSE`,
`COPYING`, or `NOTICE` file when these artifacts were built. Add the
rights holder's project license before publishing or redistributing
this plugin outside the authorized local development environment.
