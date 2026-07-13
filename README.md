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

The `codex/drpa-next-platform` branch is the architectural reset. The first milestone establishes the new shell, design system, host protocol, package model, and compatibility plan before legacy behavior is removed.

Offline machines use native sealed runtime bundles, not the source-tree wheel cache. See [`offline/README.md`](offline/README.md) for the four-platform support matrix, exact dependency policy, air-gap verification and GitHub release process.
