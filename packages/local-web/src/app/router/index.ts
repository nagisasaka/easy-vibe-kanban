import { createRouter } from '@tanstack/react-router';
import { routeTree } from '@web/routeTree.gen';
import { routeLoadOptions } from '@/shared/components/RouteLoadState';

export const router = createRouter({ routeTree, ...routeLoadOptions });

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}
