# DRPA Next

DRPA Next is a cross-platform code package runtime and operations workspace. It is designed for teams that need to distribute, configure, run, observe, and govern automation packages without asking end users to manage language runtimes.

The product is being rebuilt around three boundaries:

- **Desktop experience** — Tauri 2, React, and TypeScript.
- **Host control plane** — a Rust core that owns package installation, runtime selection, process supervision, audit history, permissions, and secrets.
- **Runtime adapters** — Python first, with Node and native command adapters planned.

The previous PySide6 desktop interface is no longer the product entry point. Its package format and task behavior are migration inputs for DRPA Next.

## Repository layout

```text
apps/desktop/             Tauri desktop application and React UI
crates/                   Rust domain and host-control crates
runtime/python/           Python runtime adapter
design-system/            Product-wide UI rules and page overrides
docs/architecture/        Architecture decisions and migration plan
legacy/                   Temporary compatibility material during migration
```

## Frontend development

The frontend can run without Tauri. When no Tauri host is detected it uses a deterministic local gateway, so visual development and component tests stay fast.

```bash
npm install
npm run dev
npm run typecheck
npm run test
npm run build
```

## Desktop development

Install the platform prerequisites from the Tauri documentation, then run:

```bash
npm run tauri:dev
```

## Product status

The `codex/drpa-next-platform` branch is the architectural reset. It now includes a Chinese-first desktop shell, real schema-v2 package installation, a local Monaco-based RPaz Studio, manifest-driven workbench parameters, and the Host-to-sealed-Python execution bridge.

Offline machines use native sealed runtime bundles, not the source-tree wheel cache. See [`offline/README.md`](offline/README.md) for the four-platform support matrix, exact dependency policy, air-gap verification and GitHub release process.

See [`docs/RPAZ_DEVELOPMENT.md`](docs/RPAZ_DEVELOPMENT.md) for the package schema, Runtime Context API, Studio workflow, manual packaging and Bing daily-image example.

Windows release policy is portable-only: no NSIS/MSI installer, no service, no shortcuts and no application-created registry keys. Public desktop release publication remains gated until the portable archive includes both the sealed Python runtime and Microsoft Fixed Version WebView2 runtime.
