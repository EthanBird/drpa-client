# DRPA Next design system

## Product character

DRPA Next is a precision operations workspace. It should feel calm under load, trustworthy around dangerous actions, and fast for people who use it every day.

The visual direction combines:

- Swiss information hierarchy.
- Dimensional layering for dense operational surfaces.
- restrained AI-native interaction patterns for command and assistance features.
- real-time monitoring conventions for runs and logs.

Avoid decorative glass panels, neon gradients, oversized dashboard cards, emoji icons, and animation that does not communicate state.

## Design tokens

### Color roles

Dark mode is the default but light mode must be first-class.

| Role | Dark | Light |
| --- | --- | --- |
| canvas | `#090B10` | `#F4F6F8` |
| surface-1 | `#0F1219` | `#FFFFFF` |
| surface-2 | `#151A23` | `#F7F9FB` |
| surface-3 | `#1C2330` | `#EDF1F5` |
| text-1 | `#F2F5F8` | `#17202B` |
| text-2 | `#A8B1BF` | `#556171` |
| text-3 | `#707B8C` | `#778396` |
| border | `#252D3A` | `#DCE2E8` |
| accent | `#7C9CFF` | `#315ED6` |
| success | `#57D6A0` | `#087A52` |
| warning | `#F5BD62` | `#9A5B00` |
| danger | `#FF7A87` | `#C62F43` |

Status is never communicated by color alone.

### Typography

- UI: Inter Variable or the platform's modern system sans fallback.
- Code and numeric telemetry: JetBrains Mono Variable.
- Base size: 14 px; dense metadata: 12 px; page title: 24 px.
- Numeric tables use tabular numerals.

### Spacing and shape

- Four-pixel base grid.
- Primary spacing scale: 4, 8, 12, 16, 24, 32.
- Controls: 32 px dense, 36 px standard, 40 px prominent.
- Radius: 6 px controls, 10 px panels, 14 px major containers.
- Shadows are reserved for floating layers; borders and tone create normal hierarchy.

## Interaction

- Every command is keyboard reachable.
- `Ctrl/Cmd + K` opens the command palette.
- `Ctrl/Cmd + Enter` runs the focused task profile.
- Destructive actions require a meaningful confirmation that names the target.
- Hover transitions last 120-180 ms. Layout motion lasts 180-240 ms.
- Respect reduced-motion preferences.
- Focus rings remain visible and meet contrast requirements.
- Log streaming batches visual updates and never steals scroll position from a user reading older output.

## Density

The interface offers comfortable and compact density. Compact mode changes spacing and row height, not font legibility.

## Navigation

Primary navigation:

1. Overview
2. Library
3. Workbench
4. Runs
5. Automations
6. Runtime Center
7. Secrets

Settings, help, update state, and the active workspace live in the utility area.

## Accessibility and QA

- Text contrast: WCAG AA minimum.
- Pointer targets: at least 32 x 32 px for dense desktop controls.
- Icon-only controls require accessible names and tooltips.
- Do not remove outlines without replacing them.
- Test at 1024 x 640, 1280 x 720, 1440 x 900, and 1920 x 1080.
- Empty, loading, error, permission-denied, offline, and high-volume states are designed states.
