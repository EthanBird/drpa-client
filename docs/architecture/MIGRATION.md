# Legacy migration contract

This document defines what can change immediately and what must remain available until parity is proven.

## Active product boundary

The Tauri application in `apps/desktop` is the only new product surface. The PySide6 UI is frozen: it receives no new product features and contributes no components to the new design system.

The legacy implementation remains in the repository temporarily because it is executable documentation for `.rpaz` v1 packages and existing task behavior. Keeping it during migration is not an architectural dependency on PySide6.

## Compatibility gates

The old entry point can be removed after automated fixtures prove all of the following:

1. A v1 package can be inspected without executing package code.
2. Manifest paths cannot escape the extracted package root.
3. Install is transactional and a failed environment build leaves the active version untouched.
4. Plaintext secret values are converted into secret-store references before a profile is persisted.
5. A package can emit logs, progress, artifacts and a final result through the versioned protocol.
6. Cancellation terminates the complete worker process tree on Windows, macOS and Linux.
7. Existing profiles and run history can be imported, with a dry-run report before mutation.

## Data handling

- The migration reads the legacy `.drpa-data` directory but never edits it in place.
- Import writes into a staging database and package store.
- A migration receipt records source hashes, conversions, warnings and skipped records.
- Rollback means deleting the new staging/imported state; it never attempts to reconstruct legacy files.
- Secret-looking values found in profiles or historical configuration are redacted from receipts and logs.

## Delivery sequence

| Milestone | Outcome | Legacy state |
| --- | --- | --- |
| M1 Platform | New shell, protocol, manifest validation and runtime adapter | Available, frozen |
| M2 Package lifecycle | Transactional install/update/remove and immutable environments | Available, frozen |
| M3 Execution parity | Supervision, secrets, artifacts, scheduling and history | Available behind migration tool |
| M4 Release | Signed cross-platform installers and updater | Removed from default product |
