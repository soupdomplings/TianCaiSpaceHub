"""Check or apply the mini account integration to an explicit site source checkout."""
import argparse
from pathlib import Path
import shutil


def prepare(root):
    files = {}
    def edit(name, old, replacement):
        path = Path(name)
        text = files[path].decode() if path in files else (root / path).read_text(encoding="utf8")
        if text.count(old) != 1 or replacement in text:
            raise ValueError(f"站点基线不符或已应用补丁：{name}")
        files[path] = text.replace(old, replacement).encode()

    auth = "backend/internal/server/routes/auth.go"
    edit(auth, "\t// 公开接口\n", """\tv1.POST("/mini-bridge/:op", h.MiniBridge(redisClient))

\t// 公开接口
""")
    edit(auth, '\t\tauthenticated.GET("/auth/me", h.Auth.GetCurrentUser)',
         '\t\tauthenticated.POST("/auth/mini/bind-code", rateLimiter.LimitWithOptions("mini-bind-code", 10, time.Minute, middleware.RateLimitOptions{FailureMode: middleware.RateLimitFailClose}), h.Auth.MiniIssue(redisClient))\n\t\tauthenticated.GET("/auth/me", h.Auth.GetCurrentUser)')
    edit("backend/internal/service/payment_service.go", "type CreateOrderRequest struct {",
         "type CreateOrderRequest struct {\n\tminiMerchantID string // populated only by the authenticated mini program bridge")
    edit("backend/internal/service/payment_order.go", "outTradeNo, err := s.allocateOutTradeNo(ctx, tx)",
         """outTradeNo := req.miniMerchantID
    if outTradeNo == "" {
        outTradeNo, err = s.allocateOutTradeNo(ctx, tx)
    }""")
    vue = "frontend/src/views/user/ProfileView.vue"
    edit(vue, "      <ProfilePasswordForm />", "      <MiniBindingCard />\n      <ProfilePasswordForm />")
    edit(vue, "import ProfileInfoCard from", "import MiniBindingCard from '@/components/user/profile/MiniBindingCard.vue'\nimport ProfileInfoCard from")
    overlay = Path(__file__).resolve().parent / "overlay"
    for source in overlay.rglob("*"):
        if source.is_file():
            dest = source.relative_to(overlay)
            if (root / dest).exists():
                raise ValueError(f"目标文件已存在：{dest}")
            files[dest] = source.read_bytes()
    return files


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    root = args.source.resolve(strict=True)
    changes = prepare(root)
    if args.apply and any((root / p).with_suffix(p.suffix + ".before-mini").exists() for p in changes):
        raise SystemExit("存在已有备份，请先核对")
    for relative, data in changes.items():
        dest = root / relative
        if args.apply:
            if dest.exists():
                shutil.copyfile(dest, dest.with_suffix(dest.suffix + ".before-mini"))
            dest.parent.mkdir(parents=True, exist_ok=True)
            staged = dest.with_suffix(dest.suffix + ".mini.tmp")
            staged.write_bytes(data)
            staged.replace(dest)
        print("APPLIED" if args.apply else "CHECKED", relative)
