package handler

import (
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/Wei-Shaw/sub2api/internal/pkg/response"
	middleware "github.com/Wei-Shaw/sub2api/internal/server/middleware"
	"github.com/Wei-Shaw/sub2api/internal/service"
	"github.com/gin-gonic/gin"
	"github.com/redis/go-redis/v9"
)

type miniBinding struct {
	UID   int64
	Stamp string
}

func miniStamp(u *service.User) string {
	h := sha256.Sum256([]byte(strconv.FormatInt(int64(u.TokenVersion), 10) + "\n" + u.PasswordHash + "\n" + u.Email))
	return hex.EncodeToString(h[:])
}

func miniRandom(n int) (string, error) {
	data := make([]byte, n)
	_, err := rand.Read(data)
	return base64.RawURLEncoding.EncodeToString(data), err
}

func miniKey(kind, token string) string {
	h := sha256.Sum256([]byte(token))
	return "mini:v1:" + kind + ":" + hex.EncodeToString(h[:])
}

func (h *AuthHandler) MiniIssue(cache *redis.Client) gin.HandlerFunc {
	return func(c *gin.Context) {
		c.Header("Cache-Control", "no-store")
		subject, ok := middleware.GetAuthSubjectFromContext(c)
		if !ok {
			response.Unauthorized(c, "请先登录")
			return
		}
		u, err := h.userService.GetByID(c.Request.Context(), subject.UserID)
		if err != nil {
			response.ErrorFrom(c, err)
			return
		}
		if ensureLoginUserActive(u) != nil || h.ensureBackendModeAllowsUser(c.Request.Context(), u) != nil {
			response.Forbidden(c, "该账户不可用")
			return
		}
		code, err := miniRandom(12)
		if err != nil {
			response.InternalError(c, "绑定码暂不可用")
			return
		}
		data, _ := json.Marshal(miniBinding{u.ID, miniStamp(u)})
		if err = cache.Set(c.Request.Context(), miniKey("code", code), data, 3*time.Minute).Err(); err != nil {
			response.InternalError(c, "绑定码暂不可用")
			return
		}
		response.Success(c, gin.H{"code": code})
	}
}

func (h *Handlers) MiniBridge(cache *redis.Client) gin.HandlerFunc {
	return func(c *gin.Context) {
		c.Header("Cache-Control", "no-store")
		key := os.Getenv("SQUAD_BRIDGE_SECRET")
		if len(key) < 32 || subtle.ConstantTimeCompare([]byte(c.GetHeader("Authorization")), []byte("Bearer "+key)) != 1 {
			response.Unauthorized(c, "未获授权")
			return
		}
		c.Request.Body = http.MaxBytesReader(c.Writer, c.Request.Body, 4096)
		var r struct {
			Link, Code, Merchant, Trade string
			Amount, Payable, Credit     float64
			Cents                       int64
		}
		if c.ShouldBindJSON(&r) != nil {
			response.BadRequest(c, "输入无效")
			return
		}
		op, ctx := c.Param("op"), c.Request.Context()
		if op == "settle" {
			if err := h.Payment.paymentService.MiniSettle(ctx, r.Merchant, r.Trade, r.Cents); err != nil {
				response.Error(c, 409, "订单尚未完成入账")
				return
			}
			response.Success(c, gin.H{"ok": true})
			return
		}
		var record []byte
		var err error
		if op == "redeem" {
			if strings.TrimSpace(r.Code) == "" {
				response.BadRequest(c, "缺少绑定码")
				return
			}
			record, err = cache.GetDel(ctx, miniKey("code", r.Code)).Bytes()
		} else {
			record, err = cache.Get(ctx, miniKey("link", r.Link)).Bytes()
		}
		var binding miniBinding
		if errors.Is(err, redis.Nil) {
			response.Error(c, 410, "绑定已失效")
			return
		}
		if err != nil || json.Unmarshal(record, &binding) != nil {
			response.InternalError(c, "绑定服务暂不可用")
			return
		}
		u, err := h.Auth.userService.GetByID(ctx, binding.UID)
		if err != nil {
			response.ErrorFrom(c, err)
			return
		}
		if !strings.EqualFold(binding.Stamp, miniStamp(u)) || ensureLoginUserActive(u) != nil || h.Auth.ensureBackendModeAllowsUser(ctx, u) != nil {
			response.Error(c, 410, "请重新登录并绑定")
			return
		}
		switch op {
		case "redeem":
			link, err := miniRandom(32)
			if err != nil || cache.Set(ctx, miniKey("link", link), record, 90*24*time.Hour).Err() != nil {
				response.InternalError(c, "绑定暂不可用")
				return
			}
			response.Success(c, gin.H{"link": link})
		case "overview":
			list, err := h.Announcement.announcementService.ListForUser(ctx, u.ID, false)
			if err != nil {
				response.ErrorFrom(c, err)
				return
			}
			now := time.Now()
			trend, err := h.Usage.usageService.GetUserUsageTrendByUserID(ctx, u.ID, now.AddDate(0, 0, -7), now, "day")
			if err != nil {
				response.ErrorFrom(c, err)
				return
			}
			announcements := []gin.H{}
			for _, item := range list {
				a := item.Announcement
				announcements = append(announcements, gin.H{"id": strconv.FormatInt(a.ID, 10), "title": a.Title, "content": a.Content})
			}
			usage := []gin.H{}
			for _, point := range trend {
				usage = append(usage, gin.H{"id": point.Date, "label": point.Date, "amount": point.ActualCost})
			}
			response.Success(c, gin.H{"balance": u.Balance, "announcements": announcements, "usage": usage})
		case "quote", "create":
			if (op == "quote" && r.Merchant != "") || (op == "create" && r.Merchant == "") {
				response.BadRequest(c, "订单信息无效")
				return
			}
			quote, err := h.Payment.paymentService.MiniQuoteOrCreate(ctx, u.ID, r.Amount, r.Merchant, service.MiniQuote{Payable: r.Payable, Credit: r.Credit})
			if err != nil {
				response.Error(c, 409, "报价或订单已变更")
				return
			}
			response.Success(c, quote)
		default:
			response.NotFound(c, "接口不存在")
		}
	}
}
