import { createContext, useContext, type ReactNode } from "react";

import type { NavigationId } from "../domain/models";

const NavigationSurfaceActiveContext = createContext(true);

export function NavigationSurface({ id, activeId, children }: { id: NavigationId; activeId: NavigationId; children: ReactNode }) {
  const active = id === activeId;
  return (
    <NavigationSurfaceActiveContext.Provider value={active}>
      <div
        className="navigation-page"
        data-navigation-page={id}
        hidden={!active}
        inert={!active}
        aria-hidden={!active}
      >
        {children}
      </div>
    </NavigationSurfaceActiveContext.Provider>
  );
}

export function useNavigationSurfaceActive(): boolean {
  return useContext(NavigationSurfaceActiveContext);
}
