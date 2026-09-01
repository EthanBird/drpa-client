import type { NavigationId } from "../domain/models";

export type NavigationMode = "normal" | "developer" | "custom";

export const CONFIGURABLE_NAVIGATION_IDS = [
  "overview",
  "library",
  "studio",
  "data",
  "workbench",
  "runs",
  "automations",
  "localDify",
  "agent",
  "knowledgeBase",
  "opticalTransfer",
  "extensionTools",
  "plugins",
  "runtimes",
  "secrets",
  "docs",
] as const satisfies readonly NavigationId[];

export const NORMAL_NAVIGATION_IDS = [
  "overview",
  "agent",
  "workbench",
  "runs",
] as const satisfies readonly NavigationId[];

const configurableNavigationSet = new Set<NavigationId>(CONFIGURABLE_NAVIGATION_IDS);

export function normalizeCustomNavigationIds(ids: readonly NavigationId[]): NavigationId[] {
  const selected = new Set(ids.filter((id) => configurableNavigationSet.has(id)));
  return CONFIGURABLE_NAVIGATION_IDS.filter((id) => selected.has(id));
}

export function getVisibleNavigationIds(
  mode: NavigationMode,
  customVisibleNavigationIds: readonly NavigationId[],
): Set<NavigationId> {
  if (mode === "normal") return new Set(NORMAL_NAVIGATION_IDS);
  if (mode === "developer") return new Set(CONFIGURABLE_NAVIGATION_IDS);
  return new Set(normalizeCustomNavigationIds(customVisibleNavigationIds));
}

export function isNavigationVisible(
  id: NavigationId,
  mode: NavigationMode,
  customVisibleNavigationIds: readonly NavigationId[],
): boolean {
  // Settings is the permanent recovery entry. Visibility preferences never
  // disable a page or its backend capability; they only hide navigation entry points.
  return id === "settings" || getVisibleNavigationIds(mode, customVisibleNavigationIds).has(id);
}
