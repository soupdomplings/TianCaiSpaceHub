//! Classify connection failures without forwarding upstream URLs or credentials.
use std::error::Error;

pub(super) const DNS_HINT: &str =
    "模型连接时域名解析失败（DNS）。请核对模型服务地址、这台电脑的网络和代理设置，恢复后重新发送";

pub(super) fn mentions_dns(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "dns error",
        "dns lookup failed",
        "failed to lookup address information",
        "no such host is known",
        "os error 11001",
        "不知道这样的主机",
        "域名解析失败",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub(crate) fn model_transport_error(error: reqwest::Error) -> anyhow::Error {
    if error.is_connect() {
        let mut cause = error.source();
        while let Some(detail) = cause {
            if mentions_dns(&detail.to_string())
                || detail
                    .downcast_ref::<std::io::Error>()
                    .and_then(std::io::Error::raw_os_error)
                    == Some(11001)
            {
                return anyhow::Error::msg(DNS_HINT);
            }
            cause = detail.source();
        }
    }
    anyhow::anyhow!("天才搭档模型 API 连接中断，请稍后重试")
}
