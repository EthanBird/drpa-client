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

Offline machines use a native sealed runtime bundle, not the source-tree wheel cache. The current delivery is Windows x64 only. See [`offline/README.md`](offline/README.md) for the exact dependency policy, air-gap verification and GitHub release process.

See [`docs/RPAZ_DEVELOPMENT.md`](docs/RPAZ_DEVELOPMENT.md) for the package schema, Runtime Context API, direct-run Studio workflow, notebook development and Bing daily-image example. The source-based Jupyter design and supported feature boundary are recorded in [`docs/JUPYTER_INTEGRATION.md`](docs/JUPYTER_INTEGRATION.md); enabled and intentionally unavailable UI actions are listed in [`docs/UI_INTERACTION_AUDIT.md`](docs/UI_INTERACTION_AUDIT.md).

Windows releases use a guided, per-user NSIS installer that performs file extraction and creates `.lnk` shortcuts only. It does not read or write application registry keys, does not register an uninstaller, and refuses installation on the Windows system drive. The sealed Python runtime, Fixed Version WebView2, projects, packages, run history, browser state and settings all remain beside the application.
