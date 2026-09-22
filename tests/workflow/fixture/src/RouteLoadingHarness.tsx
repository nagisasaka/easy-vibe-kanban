import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  lazyRouteComponent,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router';
import { routeLoadOptions } from '../../../../packages/web-core/src/shared/components/RouteLoadState';

const root = createRootRoute({ component: Outlet });
const route = createRoute({
  getParentRoute: () => root,
  path: '/lazy',
  component: lazyRouteComponent(async () => {
    await new Promise((resolve) => setTimeout(resolve, 500));
    if (new URLSearchParams(window.location.search).has('fail')) {
      throw new Error('fixture chunk unavailable');
    }
    return { default: () => <p>Lazy route content</p> };
  }),
});
const router = createRouter({
  routeTree: root.addChildren([route]),
  history: createMemoryHistory({ initialEntries: ['/lazy'] }),
  ...routeLoadOptions,
});

export function RouteLoadingHarness() {
  return <RouterProvider router={router} />;
}
