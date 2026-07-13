# Workbench page override

The Workbench is the primary daily-use surface. It uses a stable three-region layout:

```text
package/profile rail | task configuration and run controls | live activity inspector
```

Rules:

- The center region receives at least 44% of available width.
- The inspector can collapse but the run status remains visible in the top bar.
- Parameter forms group required, optional, and advanced inputs.
- Secret values are represented as named references, never as reusable plaintext fields.
- Logs use monospace typography, level markers, timestamps, search, pause-follow, and export.
- The primary Run action remains in a predictable location and displays the selected profile name.
- Package trust and capability warnings appear before the Run action, not after failure.
