//! A page visit owns its topology. Polls update evidence without deleting or
//! sorting nodes; completed nodes fade and are discarded on the next visit.
use super::visual::{RuntimeClient, RuntimeMcp, RuntimeRoute};
use crate::desktop::clients::ClientId;
use serde_json::Value;
use std::collections::BTreeMap;

struct Held<T> {
    key: String,
    value: T,
    seen: bool,
    ended: Option<i64>,
    evidence: i64,
}
#[derive(Default)]
pub(super) struct RuntimeScene {
    entered: i64,
    snapshot_at: i64,
    clients: Vec<Held<RuntimeClient>>,
    routes: Vec<Held<RuntimeRoute>>,
    mcps: Vec<Held<RuntimeMcp>>,
}
pub(super) struct SceneView {
    pub clients: Vec<RuntimeClient>,
    pub routes: Vec<(String, RuntimeRoute)>,
    pub mcps: Vec<(String, bool, RuntimeMcp)>,
}
fn fade(ended: Option<i64>, now: i64) -> f64 {
    let elapsed = ended.map(|at| now.saturating_sub(at)).unwrap_or(0);
    // Hold for three seconds, ease out over two, keep a readable quiet node.
    let p = ((elapsed - 3000) as f64 / 2000.).clamp(0., 1.);
    1. - p * p * (3. - 2. * p) * 0.6
}
fn touch<T>(rows: &mut Vec<Held<T>>, key: String, value: T, ended: Option<i64>, evidence: i64) {
    if let Some(row) = rows.iter_mut().find(|r| r.key == key) {
        row.value = value;
        row.seen = true;
        // A repeated recent-completion poll must not restart the fade clock.
        row.ended = match (row.ended, ended) {
            (Some(a), Some(b)) => Some(if evidence > row.evidence { b } else { a.min(b) }),
            (_, b) => b,
        };
        row.evidence = evidence;
    } else if rows.len() < 256 {
        rows.push(Held {
            key,
            value,
            seen: true,
            ended,
            evidence,
        });
    }
}
fn finish_missing<T>(rows: &mut [Held<T>], now: i64) {
    for row in rows {
        if !row.seen && row.ended.is_none() {
            row.ended = Some(now);
        }
    }
}
impl RuntimeScene {
    pub fn reset(&mut self, now: i64) {
        *self = Self {
            entered: now,
            ..Default::default()
        };
    }
    pub fn observe(&mut self, data: &Value) {
        let now = data["at"].as_i64().unwrap_or(0);
        if now < self.entered || now < self.snapshot_at {
            return;
        }
        self.snapshot_at = now;
        for row in &mut self.clients {
            row.seen = false;
        }
        for row in &mut self.routes {
            row.seen = false;
        }
        for row in &mut self.mcps {
            row.seen = false;
        }
        let empty = vec![];
        let calls = data["mcpCalls"].as_array().unwrap_or(&empty);
        for client in data["clients"].as_array().unwrap_or(&empty) {
            let Some(id) = ClientId::ALL
                .into_iter()
                .find(|id| client["client"] == id.key())
            else {
                continue;
            };
            let requests = client["requests"].as_array().unwrap_or(&empty);
            let tools = calls
                .iter()
                .filter(|c| c["client"] == id.key() && c["active"] == true)
                .count();
            let busy = requests.len() + tools;
            if client["opened"] != true && busy == 0 {
                continue;
            }
            touch(
                &mut self.clients,
                id.key().into(),
                RuntimeClient {
                    id,
                    state: if busy > 0 {
                        format!("{busy} 个请求处理中")
                    } else {
                        "已打开 · 空闲".into()
                    },
                    busy: busy > 0,
                    flowing: tools > 0 || requests.iter().any(|r| is_flowing(r, now)),
                    opacity: 1.,
                },
                None,
                now,
            );
        }
        let mut routes: BTreeMap<(String, String), Vec<&Value>> = BTreeMap::new();
        for r in data["requests"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .chain(data["recentRequests"].as_array().unwrap_or(&empty))
        {
            if r["phase"] == "ended" && r["lastEvidenceAt"].as_i64().unwrap_or(0) <= self.entered {
                continue;
            }
            if let (Some(route), Some(model)) = (r["route"].as_str(), r["model"].as_str()) {
                routes
                    .entry((route.into(), model.into()))
                    .or_default()
                    .push(r);
            }
            if let Some(id) = ClientId::ALL.into_iter().find(|id| r["client"] == id.key()) {
                if !self.clients.iter().any(|c| c.key == id.key() && c.seen) {
                    let ended = (r["phase"] == "ended")
                        .then(|| r["lastEvidenceAt"].as_i64().unwrap_or(now));
                    touch(
                        &mut self.clients,
                        id.key().into(),
                        RuntimeClient {
                            id,
                            state: if ended.is_some() {
                                "本页已使用"
                            } else {
                                "请求处理中"
                            }
                            .into(),
                            busy: ended.is_none(),
                            flowing: is_flowing(r, now),
                            opacity: 1.,
                        },
                        ended,
                        now,
                    );
                }
            }
        }
        for ((route, model), requests) in routes {
            let count = requests.iter().filter(|r| r["phase"] != "ended").count();
            let evidence = requests
                .iter()
                .filter_map(|r| r["lastEvidenceAt"].as_i64())
                .max()
                .unwrap_or(now);
            touch(
                &mut self.routes,
                format!("{}:{route}{model}", route.len()),
                RuntimeRoute {
                    route,
                    model,
                    channel: requests[0]["channel"].as_str().unwrap_or("渠道").into(),
                    detail: if count > 0 {
                        format!("{count} 个请求")
                    } else {
                        "本次请求已结束".into()
                    },
                    active: count > 0,
                    flowing: requests.iter().any(|r| is_flowing(r, now)),
                    opacity: 1.,
                },
                (count == 0).then_some(evidence),
                evidence,
            );
        }
        let mut tools: BTreeMap<(String, bool), Vec<&Value>> = BTreeMap::new();
        for call in calls {
            if call["active"] != true && call["at"].as_i64().unwrap_or(0) <= self.entered {
                continue;
            }
            if let Some(id) = call["resource"].as_str() {
                tools
                    .entry((id.into(), call["dumpling"] == true))
                    .or_default()
                    .push(call);
            }
        }
        for ((id, dumpling), calls) in tools {
            let key = format!("{}:{id}", if dumpling { "dumpling" } else { "library" });
            let count = calls.iter().filter(|c| c["active"] == true).count();
            let at = calls
                .iter()
                .filter_map(|c| c["at"].as_i64())
                .max()
                .unwrap_or(now);
            touch(
                &mut self.mcps,
                key,
                RuntimeMcp {
                    name: calls[0]["name"].as_str().unwrap_or("MCP").into(),
                    detail: if count > 0 {
                        format!("{count} 个调用")
                    } else {
                        "本次调用已结束".into()
                    },
                    active: count > 0,
                    flowing: count > 0,
                    opacity: 1.,
                },
                (count == 0).then_some(at),
                at,
            );
        }
        finish_missing(&mut self.clients, now);
        finish_missing(&mut self.routes, now);
        finish_missing(&mut self.mcps, now);
    }
    pub fn view(&self, now: i64, motion: bool) -> SceneView {
        let fresh = now.saturating_sub(self.snapshot_at) < 8000;
        let ended = |at: Option<i64>| {
            if fresh {
                at
            } else {
                Some(at.unwrap_or(self.snapshot_at + 8000))
            }
        };
        let clients = self
            .clients
            .iter()
            .map(|row| {
                let mut v = row.value.clone();
                let end = ended(row.ended);
                v.opacity = fade(end, now);
                v.busy &= end.is_none();
                v.flowing &= fresh && row.ended.is_none() && motion;
                if row.ended.is_some() {
                    v.state = "本页已使用".into();
                }
                if !fresh {
                    v.state = "等待状态更新".into();
                }
                v
            })
            .collect();
        let routes = self
            .routes
            .iter()
            .map(|row| {
                let mut v = row.value.clone();
                let end = ended(row.ended);
                v.active &= end.is_none();
                let evidence_fresh =
                    fresh && row.ended.is_none() && now.saturating_sub(row.evidence) < 5000;
                v.flowing &= evidence_fresh && motion;
                v.opacity = fade(
                    end.or_else(|| (!evidence_fresh && v.active).then_some(row.evidence + 5000)),
                    now,
                );
                if row.ended.is_some() {
                    v.detail = "本次请求已结束".into();
                }
                if !fresh {
                    v.detail = "等待状态更新".into();
                } else if v.active && !evidence_fresh {
                    v.detail = "等待响应".into();
                }
                (v.route.clone(), v)
            })
            .collect();
        let mcps = self
            .mcps
            .iter()
            .map(|row| {
                let mut v = row.value.clone();
                let end = ended(row.ended);
                v.opacity = fade(end, now);
                v.active &= end.is_none();
                v.flowing &= fresh && row.ended.is_none() && motion;
                if row.ended.is_some() && v.detail != "已使用" {
                    v.detail = "调用已结束".into();
                }
                if !fresh {
                    v.detail = "等待状态更新".into();
                }
                let (kind, id) = row
                    .key
                    .split_once(':')
                    .unwrap_or(("library", row.key.as_str()));
                (id.into(), kind == "dumpling", v)
            })
            .collect();
        SceneView {
            clients,
            routes,
            mcps,
        }
    }
}
fn is_flowing(r: &Value, now: i64) -> bool {
    now.saturating_sub(r["lastEvidenceAt"].as_i64().unwrap_or(0)) < 5000
        && matches!(
            r["phase"].as_str(),
            Some("routing" | "upstream" | "streaming")
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn reduced_motion_keeps_live_evidence_bright_without_animation() {
        let mut scene = RuntimeScene::default();
        scene.reset(100);
        scene.observe(&json!({"at":8000,"requests":[{"route":"r","model":"m","phase":"streaming","lastEvidenceAt":7999}]}));
        let view = scene.view(8000, false);
        let route = &view.routes[0].1;
        assert!(route.active);
        assert!(!route.flowing);
        assert_eq!(route.opacity, 1.);
        assert_ne!(route.detail, "等待状态更新");
    }
    #[test]
    fn parallel_same_model_keeps_route_identity_fades_and_reenters_cleanly() {
        let mut scene = RuntimeScene::default();
        scene.reset(100);
        let route = |name: &str| json!({"route":name,"channel":name,"model":"same","phase":"streaming","lastEvidenceAt":200});
        scene.observe(&json!({"at":200,"requests":[route("b"),route("a")]}));
        assert_eq!(scene.view(200, true).routes.len(), 2);
        scene.observe(&json!({"at":500,"requests":[]}));
        let end = scene.view(500, true);
        assert!(
            end.routes
                .iter()
                .all(|(_, r)| !r.flowing && r.opacity == 1.)
        );
        assert!(
            scene
                .view(3500, true)
                .routes
                .iter()
                .all(|(_, r)| r.opacity == 1.)
        );
        assert!(
            scene
                .view(5500, true)
                .routes
                .iter()
                .all(|(_, r)| r.opacity <= 0.41)
        );
        assert_eq!(scene.view(5500, true).routes.len(), 2);
        scene.observe(&json!({"at":5600,"requests":[route("b")]}));
        let resumed = scene.view(5600, true);
        assert_eq!(resumed.routes[0].0, "a");
        assert_eq!(resumed.routes[1].0, "b");
        assert_eq!(resumed.routes[1].1.opacity, 1.);
        scene.reset(6000);
        scene.observe(&json!({"at":6100,"recentRequests":[{"route":"a","model":"same","phase":"ended","lastEvidenceAt":5900}]}));
        assert!(scene.view(6100, true).routes.is_empty());
    }
    #[test]
    fn configured_mcp_stays_hidden_until_an_observed_tool_call() {
        let mut scene = RuntimeScene::default();
        scene.reset(100);
        scene.observe(&json!({"at":200,"clients":[{"client":"codex","opened":true,"mcps":[{"id":"file","name":"fixture"}]}]}));
        assert!(scene.view(200, true).mcps.is_empty());
        scene.observe(&json!({"at":300,"mcpCalls":[{"resource":"profile","name":"fixture","client":"workbuddy","dumpling":true,"active":true,"at":300}]}));
        let active = scene.view(300, true);
        assert_eq!(active.mcps.len(), 1);
        assert!(active.mcps[0].2.flowing);
        assert!(active.routes.is_empty());
        assert!(!scene.view(300, false).mcps[0].2.flowing);
        assert!(!scene.view(9000, true).mcps[0].2.flowing);
        scene.observe(&json!({"at":10000,"mcpCalls":[]}));
        assert_eq!(scene.view(12000, true).mcps.len(), 1);
        scene.reset(13000);
        scene.observe(&json!({"at":13100,"clients":[{"client":"codex","opened":true,"mcps":[{"id":"file","name":"fixture"}]}],"mcpCalls":[{"resource":"profile","name":"fixture","client":"workbuddy","dumpling":true,"active":false,"at":12900}]}));
        assert!(scene.view(13100, true).mcps.is_empty());
    }
    #[test]
    fn fast_completed_request_is_visible_without_restarting_its_fade() {
        let mut scene = RuntimeScene::default();
        scene.reset(100);
        let mut data = json!({"at":1000,"recentRequests":[{"route":"r","model":"fast","phase":"ended","lastEvidenceAt":500}]});
        scene.observe(&data);
        assert!(!scene.view(1000, true).routes[0].1.flowing);
        data["at"] = json!(4800);
        scene.observe(&data);
        assert!(scene.view(4800, true).routes[0].1.opacity < 0.7);
        data["at"] = json!(5200);
        data["recentRequests"] = json!([]);
        scene.observe(&data);
        assert!(scene.view(6000, true).routes[0].1.opacity <= 0.41);
        data["at"] = json!(7000);
        data["recentRequests"] =
            json!([{ "route":"r", "model":"fast", "phase":"ended", "lastEvidenceAt":6900 }]);
        scene.observe(&data);
        assert_eq!(
            scene.view(7000, true).routes[0].1.opacity,
            1.,
            "a new fast request must revive the same node"
        );
    }
    #[test]
    fn configured_tool_call_fades_without_configuration_polls_relighting_it() {
        let mut scene = RuntimeScene::default();
        scene.reset(100);
        let mut data = json!({"at":200,
            "clients":[{"client":"codex","opened":true,"mcps":[{"id":"file","name":"fixture"}]}],
            "mcpCalls":[{"resource":"file","name":"fixture","client":"codex","dumpling":false,"active":true,"at":200}]});
        scene.observe(&data);
        assert!(scene.view(200, true).mcps[0].2.flowing);
        data["at"] = json!(500);
        data["mcpCalls"][0]["active"] = json!(false);
        data["mcpCalls"][0]["at"] = json!(450);
        scene.observe(&data);
        assert_eq!(scene.view(500, true).mcps[0].2.opacity, 1.);
        data["at"] = json!(6000);
        data["mcpCalls"] = json!([]);
        scene.observe(&data);
        assert!(scene.view(6000, true).mcps[0].2.opacity <= 0.41);
        data["at"] = json!(7000);
        data["mcpCalls"] = json!([{"resource":"file","name":"fixture","client":"codex","dumpling":false,"active":false,"at":6900}]);
        scene.observe(&data);
        assert_eq!(
            scene.view(7000, true).mcps[0].2.opacity,
            1.,
            "a distinct fast tool call revives the retained node"
        );
    }
}
