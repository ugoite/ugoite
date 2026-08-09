import { useLocation, useNavigate } from "@solidjs/router";
import type { JSX } from "solid-js";
import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import {
  buildLoginPath,
  consumePendingLoginPath,
  isProtectedRoute,
} from "~/lib/auth-route";

type AuthState = "checking" | "authenticated";

const checkingView = (
  <main class="publicShell">
    <section class="publicCard ui-muted">Checking authentication…</section>
  </main>
);

export function AuthGate(props: { children: JSX.Element }) {
  const location = useLocation();
  const navigate = useNavigate();
  const [authState, setAuthState] = createSignal<AuthState>("checking");
  const [authorizedPath, setAuthorizedPath] = createSignal<string>();
  let requestId = 0;

  onCleanup(() => ++requestId);

  createEffect(() => {
    const pathname = location.pathname;
    const search = location.search;
    const currentPath = `${pathname}${search}`;

    if (!isProtectedRoute(pathname)) {
      ++requestId;
      setAuthorizedPath(undefined);
      return;
    }

    const currentRequestId = ++requestId;
    setAuthState("checking");
    setAuthorizedPath(undefined);

    void authApi.getSession().then((session) => {
      if (currentRequestId !== requestId) return;
      if (session.authenticated) {
        const pendingLoginPath = consumePendingLoginPath();
        if (pendingLoginPath && pendingLoginPath !== currentPath) {
          navigate(pendingLoginPath, { replace: true });
          return;
        }
        setAuthState("authenticated");
        setAuthorizedPath(currentPath);
        return;
      }
      navigate(buildLoginPath(pathname, search), { replace: true });
    }).catch(() => {
      if (currentRequestId !== requestId) return;
      navigate(buildLoginPath(pathname, search), { replace: true });
    });
  });

  const isReady = () =>
    !isProtectedRoute(location.pathname) ||
    (authState() === "authenticated" &&
      authorizedPath() === `${location.pathname}${location.search}`);

  return <Show when={isReady()} fallback={checkingView}>{props.children}</Show>;
}
