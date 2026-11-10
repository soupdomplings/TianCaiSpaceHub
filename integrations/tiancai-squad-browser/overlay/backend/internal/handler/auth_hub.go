package handler

// Optional companion for TianCaiSpaceHub. No browser credentials are copied.
import (
    "crypto/rand"
    "crypto/sha256"
    "encoding/base64"
    "encoding/hex"
    "encoding/json"
    "fmt"
    "io"
    "net/http"
    "net/url"
    "strings"
    "time"

    "github.com/Wei-Shaw/sub2api/internal/handler/dto"
    "github.com/Wei-Shaw/sub2api/internal/pkg/response"
    middleware2 "github.com/Wei-Shaw/sub2api/internal/server/middleware"
    "github.com/Wei-Shaw/sub2api/internal/service"
    "github.com/gin-gonic/gin"
    "github.com/redis/go-redis/v9"
)

type hubRecord struct {
    Challenge string `json:"challenge"`
    Code string `json:"code"`
    Purpose string `json:"purpose"`
    UserID int64 `json:"user_id"`
    Fingerprint string `json:"fingerprint"`
}
type hubRequest struct {
    ID string `json:"id"`
    Challenge string `json:"challenge"`
    Verifier string `json:"verifier"`
    Purpose string `json:"purpose"`
}
func hubRead(c *gin.Context) (*hubRequest, bool) {
    c.Header("Cache-Control", "no-store")
    c.Header("Referrer-Policy", "no-referrer")
    c.Request.Body = http.MaxBytesReader(c.Writer, c.Request.Body, 2048)
    d := json.NewDecoder(c.Request.Body)
    d.DisallowUnknownFields()
    var r hubRequest
    if d.Decode(&r) != nil || d.Decode(new(any)) != io.EOF {
        response.BadRequest(c, "登录请求无效，请在 Hub 重新开始")
        return nil, false
    }
    return &r, true
}
func hubID(id string) bool {
    b, e := hex.DecodeString(id)
    return e == nil && len(b) == 32 && len(id) == 64
}
func hubChallenge(challenge string) bool {
    b, e := base64.RawURLEncoding.DecodeString(challenge)
    return e == nil && len(b) == 32 && len(challenge) == 43
}
func hubFingerprint(u *service.User) string {
    // Include the password fingerprint as well as the explicit revocation version.
    sum := sha256.Sum256([]byte(fmt.Sprintf("%d\n%s\n%s", u.TokenVersion, strings.ToLower(u.Email), u.PasswordHash)))
    return hex.EncodeToString(sum[:])
}
func hubSameOrigin(c *gin.Context) bool {
    raw := c.GetHeader("Origin")
    if raw == "" { return true } // Native authenticated Hub is not a browser.
    u, err := url.Parse(raw)
    return err == nil && u.Scheme == "https" && u.Host == c.Request.Host && u.User == nil && u.Path == ""
}
func (h *AuthHandler) hubUser(c *gin.Context) *service.User {
    subject, ok := middleware2.GetAuthSubjectFromContext(c)
    if !ok || !hubSameOrigin(c) { response.Unauthorized(c, "请先登录天才小分队"); return nil }
    u, err := h.userService.GetByID(c.Request.Context(), subject.UserID)
    if err != nil { response.ErrorFrom(c, err); return nil }
    if err = ensureLoginUserActive(u); err != nil { response.ErrorFrom(c, err); return nil }
    if err = h.ensureBackendModeAllowsUser(c.Request.Context(), u); err != nil { response.ErrorFrom(c, err); return nil }
    return u
}
func hubKey(id string) string { return "hub_auth:v1:" + id }
func (h *AuthHandler) HubCapabilities(c *gin.Context) {
    c.Header("Cache-Control", "no-store")
    response.Success(c, gin.H{"version": 1})
}
// HubBegin creates an unapproved, three-minute login request. The verifier never enters the browser.
func (h *AuthHandler) HubBegin(cache *redis.Client, approved bool) gin.HandlerFunc {
    return func(c *gin.Context) {
        r, ok := hubRead(c); if !ok { return }
        if !hubChallenge(r.Challenge) { response.BadRequest(c, "登录验证信息无效"); return }
        record := hubRecord{Challenge:r.Challenge, Purpose:"login"}
        ttl := 180 * time.Second
        if approved {
            u := h.hubUser(c); if u == nil { return }
            record.UserID, record.Fingerprint, record.Purpose = u.ID, hubFingerprint(u), "return"
            ttl = 60 * time.Second
        }
        var bytes [32]byte
        if _, e := rand.Read(bytes[:]); e != nil { response.InternalError(c, "暂时无法创建登录请求"); return }
        id := hex.EncodeToString(bytes[:])
        record.Code = fmt.Sprintf("%06d", (uint32(bytes[0])<<16 | uint32(bytes[1])<<8 | uint32(bytes[2])) % 1000000)
        data, _ := json.Marshal(record)
        if cache == nil { response.InternalError(c, "登录服务暂时不可用"); return }
        added, e := cache.SetNX(c.Request.Context(), hubKey(id), data, ttl).Result()
        if e != nil || !added { response.InternalError(c, "登录服务暂时不可用"); return }
        response.Success(c, gin.H{"id":id,"code":record.Code,"expires_in":int(ttl.Seconds())})
    }
}
func (h *AuthHandler) HubPreview(cache *redis.Client) gin.HandlerFunc {
    return func(c *gin.Context) {
        c.Header("Cache-Control", "no-store")
        if h.hubUser(c) == nil { return }
        id := c.Query("id")
        if !hubID(id) || cache == nil { response.BadRequest(c,"登录请求无效"); return }
        data, e := cache.Get(c.Request.Context(), hubKey(id)).Bytes()
        var record hubRecord
        if e != nil || json.Unmarshal(data, &record) != nil || record.Purpose != "login" {
            response.BadRequest(c,"登录请求已失效，请在 Hub 重新开始"); return
        }
        response.Success(c, gin.H{"code":record.Code})
    }
}
var hubApprove = redis.NewScript(`
local raw = redis.call('GET', KEYS[1]); if not raw then return 0 end
local r = cjson.decode(raw)
if r.purpose ~= 'login' or (r.user_id ~= 0 and tostring(r.user_id) ~= ARGV[1]) then return 0 end
if r.user_id ~= 0 then return 1 end
local ttl = redis.call('PTTL', KEYS[1]); if ttl <= 0 then return 0 end
r.user_id = tonumber(ARGV[1]); r.fingerprint = ARGV[2]
redis.call('SET', KEYS[1], cjson.encode(r), 'PX', ttl); return 1`)
func (h *AuthHandler) HubApprove(cache *redis.Client) gin.HandlerFunc {
    return func(c *gin.Context) {
        r, ok := hubRead(c); if !ok { return }
        u := h.hubUser(c); if u == nil { return }
        if !hubID(r.ID) || cache == nil { response.BadRequest(c,"登录请求无效"); return }
        n,e := hubApprove.Run(c.Request.Context(), cache, []string{hubKey(r.ID)}, u.ID, hubFingerprint(u)).Int()
        if e != nil || n != 1 { response.BadRequest(c,"登录请求已失效，请在 Hub 重新开始"); return }
        response.Success(c, gin.H{"approved":true})
    }
}
// Compare proof and purpose before consuming. Invalid proofs cannot steal or destroy a request.
var hubComplete = redis.NewScript(`
local raw = redis.call('GET', KEYS[1]); if not raw then return '' end
local r = cjson.decode(raw)
if r.challenge ~= ARGV[1] or r.purpose ~= ARGV[2] then return '' end
if r.user_id == 0 then return 'pending' end
redis.call('DEL', KEYS[1]); return raw`)
func (h *AuthHandler) HubComplete(cache *redis.Client) gin.HandlerFunc {
    return func(c *gin.Context) {
        r, ok := hubRead(c); if !ok { return }
        if !hubID(r.ID) || len(r.Verifier) != 64 || (r.Purpose != "login" && r.Purpose != "return") || cache == nil {
            response.BadRequest(c,"登录请求无效"); return
        }
        if !hubSameOrigin(c) { response.Unauthorized(c,"登录来源无效"); return }
        sum := sha256.Sum256([]byte(r.Verifier))
        raw,e := hubComplete.Run(c.Request.Context(),cache,[]string{hubKey(r.ID)},base64.RawURLEncoding.EncodeToString(sum[:]),r.Purpose).Text()
        if e != nil || raw == "" { response.BadRequest(c,"登录请求已使用或已失效，请重新开始"); return }
        if raw == "pending" { response.Success(c,gin.H{"state":"pending"}); return }
        var record hubRecord
        if json.Unmarshal([]byte(raw),&record) != nil { response.InternalError(c,"登录请求无效"); return }
        u,e := h.userService.GetByID(c.Request.Context(),record.UserID)
        if e != nil { response.ErrorFrom(c,e); return }
        if e = ensureLoginUserActive(u); e != nil { response.ErrorFrom(c,e); return }
        if e = h.ensureBackendModeAllowsUser(c.Request.Context(),u); e != nil { response.ErrorFrom(c,e); return }
        if record.Fingerprint != hubFingerprint(u) { response.Unauthorized(c,"账户已变更，请重新登录"); return }
        pair,e := h.authService.GenerateTokenPair(c.Request.Context(),u,"")
        if e != nil { response.InternalError(c,"创建独立会话失败，请重新登录"); return }
        response.Success(c,gin.H{"state":"ready","access_token":pair.AccessToken,"refresh_token":pair.RefreshToken,
            "expires_in":pair.ExpiresIn,"user":dto.UserFromService(u)})
    }
}
