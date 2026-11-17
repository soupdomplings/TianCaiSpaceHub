//! Native tunnel metadata shared by DNS and HTTP node discovery.
use crate::{
    proto::common::TunnelInfo,
    tunnel::{Tunnel, ZCPacketSink, ZCPacketStream},
};
use std::pin::Pin;

pub struct TunnelWithInfo {
    inner: Box<dyn Tunnel>,
    info: TunnelInfo,
}

impl TunnelWithInfo {
    pub fn new(inner: Box<dyn Tunnel>, info: TunnelInfo) -> Self {
        Self { inner, info }
    }
}

impl Tunnel for TunnelWithInfo {
    fn split(&self) -> (Pin<Box<dyn ZCPacketStream>>, Pin<Box<dyn ZCPacketSink>>) {
        self.inner.split()
    }

    fn info(&self) -> Option<TunnelInfo> {
        Some(self.info.clone())
    }
}
