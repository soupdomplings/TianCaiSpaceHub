"""Prepare/apply the optional site companion to an explicit source checkout.

Default is check only. No deployment, configuration, database or user data access.
"""
import argparse
from pathlib import Path
import shutil


def once(text: str, anchor: str, replacement: str) -> str:
    if text.count(anchor) != 1:
        raise ValueError(f"Source changed; review required at: {anchor[:70]!r}")
    return text.replace(anchor, replacement, 1)


def prepare(root: Path) -> dict[Path, bytes]:
    routes = Path('backend/internal/server/routes/auth.go')
    router = Path('frontend/src/router/index.ts')
    auth = (root / routes).read_text(encoding='utf-8')
    vue = (root / router).read_text(encoding='utf-8')
    if 'HubCapabilities' in auth or 'HubConnectView' in vue:
        raise ValueError('Companion markers already present; do not apply twice')
    auth = once(auth, '\t// 公开接口\n', '''\t// Hub one-use authorization. Deliberately outside request-body audit logging.
    hubPublic := v1.Group("/auth/hub")
    hubPublic.Use(servermiddleware.BackendModeAuthGuard(settingService))
    hubPublic.GET("/capabilities", h.Auth.HubCapabilities)
    hubPublic.POST("/begin", rateLimiter.LimitWithOptions("hub-begin", 10, time.Minute, middleware.RateLimitOptions{FailureMode: middleware.RateLimitFailClose}), h.Auth.HubBegin(redisClient, false))
    hubPublic.POST("/complete", rateLimiter.LimitWithOptions("hub-complete", 120, time.Minute, middleware.RateLimitOptions{FailureMode: middleware.RateLimitFailClose}), h.Auth.HubComplete(redisClient))

\t// 公开接口
''')
    auth = once(auth, '\t\tauthenticated.GET("/auth/me", h.Auth.GetCurrentUser)', '''        authenticated.POST("/auth/hub/ticket", rateLimiter.LimitWithOptions("hub-ticket", 10, time.Minute, middleware.RateLimitOptions{FailureMode: middleware.RateLimitFailClose}), h.Auth.HubBegin(redisClient, true))
        authenticated.GET("/auth/hub/preview", h.Auth.HubPreview(redisClient))
        authenticated.POST("/auth/hub/approve", rateLimiter.LimitWithOptions("hub-approve", 10, time.Minute, middleware.RateLimitOptions{FailureMode: middleware.RateLimitFailClose}), h.Auth.HubApprove(redisClient))
\t\tauthenticated.GET("/auth/me", h.Auth.GetCurrentUser)''')
    vue = once(vue, 'const routes: RouteRecordRaw[] = [', '''const routes: RouteRecordRaw[] = [
  { path: '/hub/connect', name: 'HubConnect', component: () => import('@/views/auth/HubConnectView.vue'), meta: { requiresAuth: false, title: '天才 Hub 登录' } },''')
    vue = once(vue, "  if (!requiresAuth) {", "  if (!requiresAuth) {\n    // Authorization page also accepts a one-use return ticket before login.\n    if (to.path === '/hub/connect') { next(); return }")
    result = {routes: auth.encode(), router: vue.encode()}
    overlay = Path(__file__).resolve().parent / 'overlay'
    for source in overlay.rglob('*'):
        if source.is_file():
            rel = source.relative_to(overlay)
            if (root / rel).exists():
                raise ValueError(f'Refusing to replace existing file: {rel}')
            result[rel] = source.read_bytes()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    root = args.source.resolve(strict=True)
    result = prepare(root)
    # Validate every destination before changing the first file.
    for rel in result:
        backup = (root / rel).with_name(rel.name + '.before-hub-browser')
        if args.apply and backup.exists():
            raise ValueError(f'Backup already exists: {backup}')
    for rel, data in result.items():
        dest = root / rel
        if args.apply:
            if dest.exists():
                backup = dest.with_name(dest.name + '.before-hub-browser')
                if backup.exists():
                    raise ValueError(f'Backup already exists: {backup}')
                shutil.copyfile(dest, backup)
            dest.parent.mkdir(parents=True, exist_ok=True)
            temp = dest.with_name(dest.name + '.hub-browser.tmp')
            temp.write_bytes(data)
            temp.replace(dest)
        print(('APPLIED' if args.apply else 'CHECKED'), rel.as_posix())


if __name__ == '__main__':
    main()
