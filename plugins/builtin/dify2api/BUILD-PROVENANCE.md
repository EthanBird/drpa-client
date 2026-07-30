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
```

Artifacts:

```text
windows/amd64  dify2api-server.exe  CA83AA43021FFC24E742A8170DEBB685BC8C7F9054F7BED1219E466905984242
linux/amd64    dify2api-server      E73F12531E7F30AACD0409C5028AEC6964E5DCF5D65761F06DB3D7EA2F82DAB4
```

The supplied project did not contain a project-level `LICENSE`,
`COPYING`, or `NOTICE` file when these artifacts were built. Add the
rights holder's project license before publishing or redistributing
this plugin outside the authorized local development environment.
