# 微信小程序统一连接服务

桌面和小程序共用一个经过微信后台登记的 HTTPS 域名。桌面主动轮询，手机无需接入同一个局域网。SQLite 保存微信身份、有效登录凭据的摘要、电脑和手机绑定，以及小分队的订单引用。会话请求、文件片段和审批用端到端加密封装，只在内存等待转发和响应。进程重启保留配对，进行中的请求返回未确认状态。

## 部署

1. 准备 Python 3.12 和持久数据目录，限制服务账户对目录的读写权限。进入本目录，运行 `python -m pip install -r requirements.txt`。
2. 按 [配置示例](config.example.env) 向服务进程注入环境变量。必需的值为 `MOBILE_DB`、小程序的 `WX_APP_ID` 和 `WX_APP_SECRET`。正式域名同时配置给小程序和目标 Hub。
3. 以服务账户运行 `python app.py`，通过 [NGINX 配置示例](nginx.example.conf) 提供 HTTPS。默认监听本机 8080，包括首次注册和登录的入口限流。微信后台将同一域名加入 request 合法域名。域名、证书、AppID 和微信授权服务要求见[微信开发配置](https://pay.wechatpay.cn/doc/v3/partner/4012089542)。
4. 首版使用单服务进程，一个长轮询最多 15 秒，手机等待电脑响应最多 40 秒。128 个工作线程为 48 个并发桌面轮询、40 项进行中的手机请求及登录和回复保留空间。横向扩容需要跨进程的消息传输。备份 SQLite 时使用 SQLite 备份接口或停机复制，保护备份中的站点绑定引用。升级时保留数据文件。

## 配对及撤销

- `POST /v1/hub/register` 为首次启用电脑签发持久身份。电脑生成二维码前调用 `POST /v1/hub/invite`。每个邀请码五分钟有效，一次性领取，二维码中的独立密钥只由扫码手机读取。
- `POST /v1/login` 以微信 code 换取仅本服务使用的手机登录令牌，有效期一天。微信账号身份保持稳定。登录过期时静默重新登录即可查询原配对。
- 手机调用 `POST /v1/pair` 后列为待确认。桌面的 `GET /v1/hub/poll` 返回待确认、已绑定和请求。电脑在 `POST /v1/hub/decide` 同意之前，手机请求无法到达会话。设备列表的在线状态来自实际轮询。撤销手机用 `POST /v1/hub/revoke`，云端持久删除关联，并唤醒已在等待的请求。
- 手机的 `POST /v1/rpc` 和桌面的 `POST /v1/hub/reply` 唯一接受的封包字段为 `bindingId, requestId, nonce, ciphertext`。密文和随机数使用无填充 base64url。两端的 AES-256-GCM AAD 使用 UTF-8 字面值 `tc-hub-v1|绑定ID|请求ID|req` 或 `res`。转发层按持有的令牌认证、限制包长和检查绑定归属。每个客户端写入的执行和去重状态由电脑处理。云端不重发已交付的不确定写入。

## 小分队账户和充值

部署[站点配套补丁](../integrations/tiancai-squad-mini/README.md)，配置 `SQUAD_ORIGIN` 和两端相同的高熵 `SQUAD_BRIDGE_SECRET`。用户登录网站并从个人资料生成一次性绑定码，在小程序内兑换。远程配对独立于该账户。个人公告和七日用量来自原站点的个人查询，余额以站点为准。

充值使用微信小程序虚拟支付道具直购。审批通过后再配置 OfferID、现网虚拟支付 AppKey 和已获准的终端。例如仅开通安卓时填写 `WX_PAYMENT_PLATFORMS=android`。每个充值档位必须对应微信后台实际发布的商品和以分计的总支付价。例如测试格式 `{"10":{"id":"your_published_sku","priceFen":1000}}`。站点手续费和赠金规则决定实付与到账，商品价格和站点报价不一致时不下单。完整设置见[腾讯云小程序虚拟支付资料](https://github.com/TencentCloudBase/skills/blob/main/skills/cloudbase-wechat-integration/references/virtual-payment.md)。

同一张报价只生成一个商家单号，两边以此核对原订单。微信登录身份、订单号、支付状态与实付分数匹配后，才调用站点余额订单的幂等到账流程。用户返回小程序时直接查原订单，后台每分钟补查未完成订单，即使手机关闭也能收到入账。到账后通知微信发货。本版用主动查单确认支付，未接入退款申请与退款回收，退款由原运营流程核对处理。

## 复核

运行 `python -B -m unittest discover -s mobile-relay -v`（仓库根目录）。测试使用合成微信、站点和临时数据库，覆盖设备持久绑定、拒绝跨账户访问、待处理请求撤销、密文转发、报价变化和重复查单。公网域名、微信真机、生产站点和实际扣款均待部署方实机验收。
