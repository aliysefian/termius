//! Kubernetes next to containers: pods, their logs, a shell in one, and port-forwards, through the
//! `kubectl` that is already on this computer or on one of the saved hosts. Nothing is installed anywhere,
//! and nothing reads a kubeconfig but `kubectl` itself.
//!
//! Every name that reaches `kubectl` is checked against what Kubernetes allows rather than escaped, and is
//! passed as one argument (single-quoted for a remote login shell by the transport), so a pod can't be
//! named into a command.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::containers::transport::{Output, SshShell, Transport, Utf8Chunker};
use crate::containers::{ContainerError, LogEvent, LogSink};
use crate::ssh::Target;

type KubeResult<T> = Result<T, ContainerError>;

const BIN: &str = "kubectl";
const LIST_TIMEOUT: Duration = Duration::from_secs(25);
const ACTION_TIMEOUT: Duration = Duration::from_secs(40);
/// Handed to every call, so a cluster that isn't answering ends the call instead of hanging it.
const REQUEST_TIMEOUT: &str = "--request-timeout=15s";

// -- names -----------------------------------------------------------------------------------

/// A namespace, pod, container or service name: what Kubernetes itself allows (DNS labels and subdomains).
pub fn check_name(what: &str, s: &str) -> KubeResult<()> {
    let ok = !s.is_empty()
        && s.len() <= 253
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars().last().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
    if ok {
        Ok(())
    } else {
        Err(ContainerError::Invalid(format!("\"{s}\" isn't a valid {what} name")))
    }
}

/// A kubeconfig context name, which is free-form (`arn:aws:eks:…`, `gke_proj_zone_cluster`, `user@cluster`).
pub fn check_context(s: &str) -> KubeResult<()> {
    let ok = !s.is_empty()
        && s.len() <= 253
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_.:/@=,+-".contains(c));
    if ok {
        Ok(())
    } else {
        Err(ContainerError::Invalid(format!("\"{s}\" isn't a usable context name")))
    }
}

fn port(what: &str, p: u32, floor: u32) -> KubeResult<u32> {
    if (floor..=65535).contains(&p) {
        Ok(p)
    } else {
        Err(ContainerError::Invalid(format!("{what} must be between {floor} and 65535")))
    }
}

// -- what to run ------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "scope", content = "name", rename_all = "snake_case")]
pub enum Scope {
    All,
    Namespace(String),
}

fn base(context: &str) -> KubeResult<Vec<String>> {
    check_context(context)?;
    Ok(vec![REQUEST_TIMEOUT.into(), "--context".into(), context.into()])
}

fn with(mut args: Vec<String>, more: &[&str]) -> Vec<String> {
    args.extend(more.iter().map(|s| (*s).to_string()));
    args
}

pub fn pods_args(context: &str, scope: &Scope) -> KubeResult<Vec<String>> {
    let b = base(context)?;
    Ok(match scope {
        Scope::All => with(b, &["get", "pods", "-A", "-o", "json"]),
        Scope::Namespace(ns) => {
            check_name("namespace", ns)?;
            with(b, &["get", "pods", "-n", ns, "-o", "json"])
        }
    })
}

pub fn namespaces_args(context: &str) -> KubeResult<Vec<String>> {
    Ok(with(base(context)?, &["get", "namespaces", "-o", "name"]))
}

pub fn describe_args(context: &str, ns: &str, pod: &str) -> KubeResult<Vec<String>> {
    check_name("namespace", ns)?;
    check_name("pod", pod)?;
    Ok(with(base(context)?, &["describe", "pod", pod, "-n", ns]))
}

pub fn delete_args(context: &str, ns: &str, pod: &str) -> KubeResult<Vec<String>> {
    check_name("namespace", ns)?;
    check_name("pod", pod)?;
    Ok(with(base(context)?, &["delete", "pod", pod, "-n", ns, "--wait=false"]))
}

/// The kinds that can be listed besides pods. Read-only, and deliberately without Secrets: nothing here asks the
/// cluster for them, so they can't be shown, copied or logged by mistake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Deployments,
    StatefulSets,
    DaemonSets,
    Services,
    ConfigMaps,
    Jobs,
    CronJobs,
    Ingresses,
    Events,
    Nodes,
}

impl Kind {
    /// What `kubectl get` and `kubectl describe` call it.
    pub fn api_name(self) -> &'static str {
        match self {
            Kind::Deployments => "deployments",
            Kind::StatefulSets => "statefulsets",
            Kind::DaemonSets => "daemonsets",
            Kind::Services => "services",
            Kind::ConfigMaps => "configmaps",
            Kind::Jobs => "jobs",
            Kind::CronJobs => "cronjobs",
            Kind::Ingresses => "ingresses",
            Kind::Events => "events",
            Kind::Nodes => "nodes",
        }
    }

    /// Nodes belong to the cluster, not to a namespace.
    pub fn namespaced(self) -> bool {
        self != Kind::Nodes
    }
}

pub fn resources_args(context: &str, scope: &Scope, kind: Kind) -> KubeResult<Vec<String>> {
    let b = base(context)?;
    Ok(match (scope, kind.namespaced()) {
        (Scope::Namespace(ns), true) => {
            check_name("namespace", ns)?;
            with(b, &["get", kind.api_name(), "-n", ns, "-o", "json"])
        }
        (Scope::All, true) => with(b, &["get", kind.api_name(), "-A", "-o", "json"]),
        (_, false) => with(b, &["get", kind.api_name(), "-o", "json"]),
    })
}

pub fn describe_resource_args(context: &str, ns: &str, kind: Kind, name: &str) -> KubeResult<Vec<String>> {
    check_name("name", name)?;
    if kind.namespaced() {
        check_name("namespace", ns)?;
        Ok(with(base(context)?, &["describe", kind.api_name(), name, "-n", ns]))
    } else {
        Ok(with(base(context)?, &["describe", kind.api_name(), name]))
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct LogOptions {
    pub tail: u32,
    pub follow: bool,
    pub timestamps: bool,
    /// The previous instance of the container (after a crash).
    pub previous: bool,
}

pub fn logs_args(context: &str, ns: &str, pod: &str, container: Option<&str>, o: LogOptions) -> KubeResult<Vec<String>> {
    check_name("namespace", ns)?;
    check_name("pod", pod)?;
    let mut args = base(context)?;
    // The request timeout would cut a followed log off; a log is not a request.
    args.remove(0);
    args.extend(["logs".into(), pod.into(), "-n".into(), ns.into(), format!("--tail={}", o.tail.min(100_000))]);
    if let Some(c) = container {
        check_name("container", c)?;
        args.extend(["-c".into(), c.into()]);
    }
    if o.follow && !o.previous {
        args.push("-f".into());
    }
    if o.timestamps {
        args.push("--timestamps".into());
    }
    if o.previous {
        args.push("--previous".into());
    }
    Ok(args)
}

/// Where a port-forward goes: a pod, or a service.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum Target_ {
    Pod(String),
    Service(String),
}

pub fn forward_args(context: &str, ns: &str, to: &Target_, local: u32, remote: u32) -> KubeResult<Vec<String>> {
    check_name("namespace", ns)?;
    let target = match to {
        Target_::Pod(n) => {
            check_name("pod", n)?;
            format!("pod/{n}")
        }
        Target_::Service(n) => {
            check_name("service", n)?;
            format!("service/{n}")
        }
    };
    let mut args = base(context)?;
    args.remove(0);
    args.extend(["port-forward".into(), target, "-n".into(), ns.into(), "--address".into(), "127.0.0.1".into(), format!("{}:{}", port("The local port", local, 1)?, port("The remote port", remote, 1)?)]);
    Ok(args)
}

// -- what comes back ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Pod {
    pub name: String,
    pub namespace: String,
    /// What a person would call its state: Running, Pending, CrashLoopBackOff, Terminating ...
    pub status: String,
    /// "ready/total" containers, as kubectl shows.
    pub ready: String,
    pub restarts: u64,
    pub node: Option<String>,
    pub ip: Option<String>,
    /// RFC 3339, as the API gave it; the window works out the age.
    pub created: Option<String>,
    pub containers: Vec<String>,
    /// What owns it (a ReplicaSet, a Job ...), as "Kind/name".
    pub owner: Option<String>,
}

fn s(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_str().map(str::to_string)
}

/// The status line kubectl would show: a waiting or terminated reason beats the phase.
fn status_of(pod: &Value) -> String {
    if pod["metadata"].get("deletionTimestamp").is_some() {
        return "Terminating".into();
    }
    let all = ["initContainerStatuses", "containerStatuses"];
    for key in all {
        if let Some(list) = pod["status"][key].as_array() {
            for c in list {
                if let Some(r) = s(c, &["state", "waiting", "reason"]) {
                    return r;
                }
                if let Some(r) = s(c, &["state", "terminated", "reason"]) {
                    if r != "Completed" {
                        return r;
                    }
                }
            }
        }
    }
    s(pod, &["status", "phase"]).unwrap_or_else(|| "Unknown".into())
}

pub fn parse_pods(json: &str) -> Result<Vec<Pod>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let items = v["items"].as_array().ok_or("no list of pods in the answer")?;
    let mut out = Vec::with_capacity(items.len());
    for p in items {
        let Some(name) = s(p, &["metadata", "name"]) else { continue };
        let containers: Vec<String> = p["spec"]["containers"].as_array().map(|l| l.iter().filter_map(|c| s(c, &["name"])).collect()).unwrap_or_default();
        let statuses = p["status"]["containerStatuses"].as_array().cloned().unwrap_or_default();
        let ready = statuses.iter().filter(|c| c["ready"].as_bool() == Some(true)).count();
        let restarts = statuses.iter().filter_map(|c| c["restartCount"].as_u64()).sum();
        let owner = p["metadata"]["ownerReferences"].as_array().and_then(|l| l.first()).and_then(|o| Some(format!("{}/{}", s(o, &["kind"])?, s(o, &["name"])?)));
        out.push(Pod {
            name,
            namespace: s(p, &["metadata", "namespace"]).unwrap_or_default(),
            status: status_of(p),
            ready: format!("{}/{}", ready, containers.len()),
            restarts,
            node: s(p, &["spec", "nodeName"]),
            ip: s(p, &["status", "podIP"]),
            created: s(p, &["metadata", "creationTimestamp"]),
            containers,
            owner,
        });
    }
    out.sort_by(|a, b| (a.namespace.as_str(), a.name.as_str()).cmp(&(b.namespace.as_str(), b.name.as_str())));
    Ok(out)
}

pub fn parse_names(out: &str) -> Vec<String> {
    let mut v: Vec<String> = out.lines().map(|l| l.trim().rsplit('/').next().unwrap_or("").to_string()).filter(|l| !l.is_empty()).collect();
    v.sort();
    v.dedup();
    v
}

/// One row of a list of something that isn't a pod.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Resource {
    pub name: String,
    /// Empty for a cluster-wide thing such as a node.
    pub namespace: String,
    /// What a person would read first: "3/3 ready", "ClusterIP", "Warning", "Ready" ...
    pub status: String,
    pub created: Option<String>,
    /// The other columns that mean something for this kind.
    pub details: Vec<Detail>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Detail {
    pub label: String,
    pub value: String,
}

fn d(label: &str, value: impl Into<String>) -> Detail {
    Detail { label: label.into(), value: value.into() }
}

fn n(v: &Value, path: &[&str]) -> u64 {
    let mut cur = v;
    for p in path {
        match cur.get(p) {
            Some(x) => cur = x,
            None => return 0,
        }
    }
    cur.as_u64().unwrap_or(0)
}

fn list(v: &Value, path: &[&str]) -> Vec<Value> {
    let mut cur = v;
    for p in path {
        match cur.get(p) {
            Some(x) => cur = x,
            None => return Vec::new(),
        }
    }
    cur.as_array().cloned().unwrap_or_default()
}

/// Longest message kept from an event: they can be pages of text.
const MAX_MESSAGE: usize = 300;
/// Most events listed; the newest are kept.
const MAX_EVENTS: usize = 500;

fn clip(text: &str, max: usize) -> String {
    let one: String = text.lines().next().unwrap_or("").chars().take(max).collect();
    if text.lines().next().is_some_and(|l| l.chars().count() > max) {
        format!("{one}…")
    } else {
        one
    }
}

pub fn parse_resources(kind: Kind, json: &str) -> Result<Vec<Resource>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let items = v["items"].as_array().ok_or_else(|| format!("no list of {} in the answer", kind.api_name()))?;
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        let Some(name) = s(it, &["metadata", "name"]) else { continue };
        let (status, details) = match kind {
            Kind::Deployments => {
                let want = it["spec"].get("replicas").and_then(Value::as_u64).unwrap_or(1);
                (
                    format!("{}/{want} ready", n(it, &["status", "readyReplicas"])),
                    vec![d("Up to date", n(it, &["status", "updatedReplicas"]).to_string()), d("Available", n(it, &["status", "availableReplicas"]).to_string())],
                )
            }
            Kind::StatefulSets => {
                let want = it["spec"].get("replicas").and_then(Value::as_u64).unwrap_or(1);
                (format!("{}/{want} ready", n(it, &["status", "readyReplicas"])), vec![d("Service", s(it, &["spec", "serviceName"]).unwrap_or_default())])
            }
            Kind::DaemonSets => (
                format!("{}/{} ready", n(it, &["status", "numberReady"]), n(it, &["status", "desiredNumberScheduled"])),
                vec![d("Available", n(it, &["status", "numberAvailable"]).to_string())],
            ),
            Kind::Services => {
                let ports: Vec<String> = list(it, &["spec", "ports"]).iter().map(|p| format!("{}/{}", p["port"].as_u64().unwrap_or(0), p["protocol"].as_str().unwrap_or("TCP"))).collect();
                let mut external: Vec<String> = list(it, &["status", "loadBalancer", "ingress"]).iter().filter_map(|i| s(i, &["ip"]).or_else(|| s(i, &["hostname"]))).collect();
                external.extend(list(it, &["spec", "externalIPs"]).iter().filter_map(|x| x.as_str().map(str::to_string)));
                (
                    s(it, &["spec", "type"]).unwrap_or_else(|| "ClusterIP".into()),
                    vec![d("Cluster IP", s(it, &["spec", "clusterIP"]).unwrap_or_default()), d("External", external.join(", ")), d("Ports", ports.join(", "))],
                )
            }
            Kind::ConfigMaps => {
                // Key names only: a value is read on purpose, with describe, never in a list.
                let mut keys: Vec<String> = it.get("data").and_then(Value::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default();
                keys.sort();
                let count = keys.len();
                let shown = keys.into_iter().take(10).collect::<Vec<_>>().join(", ");
                (format!("{count} key{}", if count == 1 { "" } else { "s" }), vec![d("Keys", if count > 10 { format!("{shown} …") } else { shown })])
            }
            Kind::Jobs => {
                let done = n(it, &["status", "succeeded"]);
                let want = it["spec"].get("completions").and_then(Value::as_u64).unwrap_or(1);
                let conditions = list(it, &["status", "conditions"]);
                let has = |t: &str| conditions.iter().any(|c| s(c, &["type"]).as_deref() == Some(t) && s(c, &["status"]).as_deref() == Some("True"));
                let state = if has("Failed") {
                    "Failed".to_string()
                } else if has("Complete") {
                    "Complete".to_string()
                } else {
                    "Running".to_string()
                };
                (state, vec![d("Completions", format!("{done}/{want}")), d("Failed pods", n(it, &["status", "failed"]).to_string())])
            }
            Kind::CronJobs => (
                if it["spec"]["suspend"].as_bool() == Some(true) { "Suspended".into() } else { "Scheduled".into() },
                vec![d("Schedule", s(it, &["spec", "schedule"]).unwrap_or_default()), d("Last run", s(it, &["status", "lastScheduleTime"]).unwrap_or_default())],
            ),
            Kind::Ingresses => {
                let hosts: Vec<String> = list(it, &["spec", "rules"]).iter().filter_map(|r| s(r, &["host"])).collect();
                (
                    s(it, &["spec", "ingressClassName"]).unwrap_or_else(|| "default".into()),
                    vec![d("Hosts", if hosts.is_empty() { "*".to_string() } else { hosts.join(", ") })],
                )
            }
            Kind::Events => {
                let when = s(it, &["lastTimestamp"]).or_else(|| s(it, &["eventTime"])).or_else(|| s(it, &["metadata", "creationTimestamp"])).unwrap_or_default();
                (
                    s(it, &["type"]).unwrap_or_else(|| "Normal".into()),
                    vec![
                        d("Reason", s(it, &["reason"]).unwrap_or_default()),
                        d("Object", format!("{}/{}", s(it, &["involvedObject", "kind"]).unwrap_or_default(), s(it, &["involvedObject", "name"]).unwrap_or_default())),
                        d("Message", clip(&s(it, &["message"]).unwrap_or_default(), MAX_MESSAGE)),
                        d("Count", it.get("count").and_then(Value::as_u64).unwrap_or(1).to_string()),
                        d("Last seen", when),
                    ],
                )
            }
            Kind::Nodes => {
                let ready = list(it, &["status", "conditions"]).iter().find(|c| s(c, &["type"]).as_deref() == Some("Ready")).and_then(|c| s(c, &["status"]));
                let mut roles: Vec<String> = it["metadata"]["labels"].as_object().map(|o| o.keys().filter_map(|k| k.strip_prefix("node-role.kubernetes.io/").map(str::to_string)).collect()).unwrap_or_default();
                roles.sort();
                let ip = list(it, &["status", "addresses"]).iter().find(|a| s(a, &["type"]).as_deref() == Some("InternalIP")).and_then(|a| s(a, &["address"])).unwrap_or_default();
                (
                    match ready.as_deref() {
                        Some("True") => "Ready".into(),
                        Some("False") => "NotReady".into(),
                        _ => "Unknown".into(),
                    },
                    vec![d("Roles", if roles.is_empty() { "<none>".to_string() } else { roles.join(", ") }), d("Version", s(it, &["status", "nodeInfo", "kubeletVersion"]).unwrap_or_default()), d("Internal IP", ip)],
                )
            }
        };
        out.push(Resource { name, namespace: s(it, &["metadata", "namespace"]).unwrap_or_default(), status, created: s(it, &["metadata", "creationTimestamp"]), details });
    }
    if kind == Kind::Events {
        // Newest first, and not a flood.
        let seen = |r: &Resource| r.details.iter().find(|x| x.label == "Last seen").map(|x| x.value.clone()).unwrap_or_default();
        out.sort_by_key(|r| std::cmp::Reverse(seen(r)));
        out.truncate(MAX_EVENTS);
    } else {
        out.sort_by(|a, b| (a.namespace.as_str(), a.name.as_str()).cmp(&(b.namespace.as_str(), b.name.as_str())));
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct KubeInfo {
    /// The client version `kubectl` reports.
    pub version: String,
    pub contexts: Vec<String>,
    pub current: Option<String>,
}

/// Turn what `kubectl` said into a message: its own words, with a hint where one helps.
fn failure(out: &Output) -> ContainerError {
    let text = if out.stderr.trim().is_empty() { out.stdout.trim() } else { out.stderr.trim() };
    let lower = text.to_ascii_lowercase();
    if out.code == Some(127) || lower.contains("kubectl: not found") || lower.contains("kubectl: command not found") {
        return ContainerError::NotInstalled(BIN.to_string());
    }
    let hint = if lower.contains("unable to connect to the server") || lower.contains("i/o timeout") || lower.contains("context deadline exceeded") || lower.contains("connection refused") {
        "\nHint: the cluster isn't answering from here (a VPN, a stopped cluster, or an expired login)."
    } else if lower.contains("forbidden") {
        "\nHint: this account isn't allowed to do that in that namespace."
    } else if lower.contains("you must be logged in") || lower.contains("unauthorized") {
        "\nHint: the login for this context has expired."
    } else if lower.contains("no configuration has been provided") || lower.contains("current-context is not set") {
        "\nHint: there is no kubeconfig here, or no current context."
    } else {
        ""
    };
    ContainerError::Command(format!("{}{hint}", if text.is_empty() { "kubectl failed" } else { text }))
}

// -- sessions -------------------------------------------------------------------------------------

struct Session {
    transport: Transport,
}

#[derive(Default)]
pub struct KubeManager {
    sessions: Mutex<HashMap<Uuid, Arc<Session>>>,
    /// Followed logs and port-forwards: the session each belongs to, and how to stop it.
    streams: Mutex<HashMap<Uuid, (Uuid, oneshot::Sender<()>)>>,
}

impl KubeManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, id: Uuid) -> KubeResult<Arc<Session>> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned().ok_or(ContainerError::NoSession)
    }

    /// Open this computer (`via` is `None`) or a host, and read its contexts. Fails with `NotInstalled` if
    /// there is no `kubectl` there.
    pub async fn open(&self, via: Option<Target>) -> KubeResult<(Uuid, KubeInfo)> {
        let transport = match via {
            Some(t) => Transport::Ssh(Box::new(SshShell::connect(t).await?)),
            None => Transport::Local,
        };
        let v = transport.exec(BIN, &["version".into(), "--client".into(), "-o".into(), "json".into()], LIST_TIMEOUT).await?;
        if v.code != Some(0) {
            transport.close().await;
            return Err(failure(&v));
        }
        let version = serde_json::from_str::<Value>(&v.stdout).ok().and_then(|j| s(&j, &["clientVersion", "gitVersion"])).unwrap_or_else(|| "unknown".into());
        let contexts = match transport.exec(BIN, &["config".into(), "get-contexts".into(), "-o".into(), "name".into()], LIST_TIMEOUT).await {
            Ok(o) if o.code == Some(0) => parse_names(&o.stdout),
            _ => Vec::new(),
        };
        let current = match transport.exec(BIN, &["config".into(), "current-context".into()], LIST_TIMEOUT).await {
            Ok(o) if o.code == Some(0) => Some(o.stdout.trim().to_string()).filter(|c| !c.is_empty()),
            _ => None,
        };
        let id = Uuid::new_v4();
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(Session { transport }));
        Ok((id, KubeInfo { version, contexts, current }))
    }

    pub async fn close(&self, id: Uuid) {
        let session = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
        let mine: Vec<Uuid> = self.streams.lock().unwrap_or_else(|p| p.into_inner()).iter().filter(|(_, (sid, _))| *sid == id).map(|(k, _)| *k).collect();
        for k in mine {
            self.stop_stream(k);
        }
        if let Some(s) = session {
            s.transport.close().await;
        }
    }

    pub async fn close_all(&self) {
        let ids: Vec<Uuid> = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).keys().copied().collect();
        for id in ids {
            self.close(id).await;
        }
    }

    async fn run(&self, session: Uuid, args: &[String], timeout: Duration) -> KubeResult<Output> {
        let out = self.get(session)?.transport.exec(BIN, args, timeout).await?;
        if out.code == Some(0) {
            Ok(out)
        } else {
            Err(failure(&out))
        }
    }

    pub async fn pods(&self, session: Uuid, context: &str, scope: &Scope) -> KubeResult<Vec<Pod>> {
        let out = self.run(session, &pods_args(context, scope)?, LIST_TIMEOUT).await?;
        parse_pods(&out.stdout).map_err(ContainerError::Parse)
    }

    pub async fn resources(&self, session: Uuid, context: &str, scope: &Scope, kind: Kind) -> KubeResult<Vec<Resource>> {
        let out = self.run(session, &resources_args(context, scope, kind)?, LIST_TIMEOUT).await?;
        parse_resources(kind, &out.stdout).map_err(ContainerError::Parse)
    }

    pub async fn describe_resource(&self, session: Uuid, context: &str, ns: &str, kind: Kind, name: &str) -> KubeResult<String> {
        Ok(self.run(session, &describe_resource_args(context, ns, kind, name)?, LIST_TIMEOUT).await?.stdout)
    }

    pub async fn namespaces(&self, session: Uuid, context: &str) -> KubeResult<Vec<String>> {
        let out = self.run(session, &namespaces_args(context)?, LIST_TIMEOUT).await?;
        Ok(parse_names(&out.stdout))
    }

    pub async fn describe(&self, session: Uuid, context: &str, ns: &str, pod: &str) -> KubeResult<String> {
        Ok(self.run(session, &describe_args(context, ns, pod)?, LIST_TIMEOUT).await?.stdout)
    }

    pub async fn delete_pod(&self, session: Uuid, context: &str, ns: &str, pod: &str) -> KubeResult<()> {
        self.run(session, &delete_args(context, ns, pod)?, ACTION_TIMEOUT).await.map(|_| ())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_logs(self: &Arc<Self>, session: Uuid, context: &str, ns: &str, pod: &str, container: Option<&str>, o: LogOptions, sink: Arc<dyn LogSink>) -> KubeResult<Uuid> {
        let args = logs_args(context, ns, pod, container, o)?;
        self.start_stream(session, args, sink)
    }

    /// Forward a local port to a pod or service. Runs until stopped; `kubectl`'s own lines ("Forwarding
    /// from …") arrive as chunks. On a host, the port opens on that host, not on this computer.
    #[allow(clippy::too_many_arguments)]
    pub fn start_forward(self: &Arc<Self>, session: Uuid, context: &str, ns: &str, to: &Target_, local: u32, remote: u32, sink: Arc<dyn LogSink>) -> KubeResult<Uuid> {
        let args = forward_args(context, ns, to, local, remote)?;
        self.start_stream(session, args, sink)
    }

    fn start_stream(self: &Arc<Self>, session: Uuid, args: Vec<String>, sink: Arc<dyn LogSink>) -> KubeResult<Uuid> {
        let s = self.get(session)?;
        let stream_id = Uuid::new_v4();
        let (stop_tx, stop_rx) = oneshot::channel();
        self.streams.lock().unwrap_or_else(|p| p.into_inner()).insert(stream_id, (session, stop_tx));
        let me = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let chunker = Mutex::new(Utf8Chunker::default());
            let result = s
                .transport
                .stream(
                    BIN,
                    &args,
                    |bytes| {
                        let text = chunker.lock().unwrap_or_else(|p| p.into_inner()).push(bytes);
                        if !text.is_empty() {
                            sink.event(LogEvent::Chunk { text });
                        }
                    },
                    stop_rx,
                )
                .await;
            let rest = chunker.lock().unwrap_or_else(|p| p.into_inner()).finish();
            if !rest.is_empty() {
                sink.event(LogEvent::Chunk { text: rest });
            }
            me.streams.lock().unwrap_or_else(|p| p.into_inner()).remove(&stream_id);
            sink.event(match result {
                Ok(code) => LogEvent::End { code, error: None },
                Err(e) => LogEvent::End { code: None, error: Some(e.to_string()) },
            });
        });
        Ok(stream_id)
    }

    pub fn stop_stream(&self, stream_id: Uuid) {
        if let Some((_, stop)) = self.streams.lock().unwrap_or_else(|p| p.into_inner()).remove(&stream_id) {
            let _ = stop.send(());
        }
    }

    pub fn stream_count(&self) -> usize {
        self.streams.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PODS: &str = r#"{"items":[
      {"metadata":{"name":"web-7d9f-abc","namespace":"shop","creationTimestamp":"2026-10-01T10:00:00Z","ownerReferences":[{"kind":"ReplicaSet","name":"web-7d9f"}]},
       "spec":{"nodeName":"node-1","containers":[{"name":"app"},{"name":"sidecar"}]},
       "status":{"phase":"Running","podIP":"10.1.2.3","containerStatuses":[{"name":"app","ready":true,"restartCount":0},{"name":"sidecar","ready":false,"restartCount":4,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}]}},
      {"metadata":{"name":"batch-1","namespace":"jobs","deletionTimestamp":"2026-10-07T00:00:00Z"},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":true,"restartCount":1}]}},
      {"metadata":{"name":"api-0","namespace":"shop"},"spec":{"containers":[{"name":"api"}]},"status":{"phase":"Pending","initContainerStatuses":[{"name":"init","state":{"waiting":{"reason":"PodInitializing"}}}]}},
      {"metadata":{"name":"done","namespace":"jobs"},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Succeeded","containerStatuses":[{"name":"c","ready":false,"restartCount":0,"state":{"terminated":{"reason":"Completed"}}}]}},
      {"metadata":{}}
    ]}"#;

    #[test]
    fn names_are_checked_not_escaped() {
        for ok in ["web", "web-7d9f-abc", "a.b-c", "0abc", &"a".repeat(253)] {
            assert!(check_name("pod", ok).is_ok(), "{ok}");
        }
        for bad in ["", "Web", "-a", "a-", "a_b", "a b", "a;b", "$(x)", "a/b", "a'b", "../x", "a\n", &"a".repeat(254)] {
            assert!(check_name("pod", bad).is_err(), "{bad:?}");
        }
        for ok in ["minikube", "arn:aws:eks:eu-west-1:123456789012:cluster/prod", "gke_proj_europe-west1-b_main", "user@cluster.example.com", "kind-kind"] {
            assert!(check_context(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-x", "a b", "a;b", "$(id)", "`id`", "a'b", "a\"b", "a\n", "--all-namespaces"] {
            assert!(check_context(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn argument_lists() {
        assert_eq!(pods_args("kind-kind", &Scope::All).unwrap(), ["--request-timeout=15s", "--context", "kind-kind", "get", "pods", "-A", "-o", "json"]);
        assert_eq!(pods_args("c", &Scope::Namespace("shop".into())).unwrap(), ["--request-timeout=15s", "--context", "c", "get", "pods", "-n", "shop", "-o", "json"]);
        assert!(pods_args("c", &Scope::Namespace("a;b".into())).is_err());
        assert_eq!(describe_args("c", "shop", "web").unwrap()[3..], ["describe", "pod", "web", "-n", "shop"]);
        assert_eq!(delete_args("c", "shop", "web").unwrap().last().unwrap(), "--wait=false");
        let o = LogOptions { tail: 200, follow: true, timestamps: true, previous: false };
        assert_eq!(logs_args("c", "shop", "web", Some("app"), o).unwrap(), ["--context", "c", "logs", "web", "-n", "shop", "--tail=200", "-c", "app", "-f", "--timestamps"]);
        // A followed log has no request timeout, and "previous" doesn't follow.
        assert!(!logs_args("c", "s", "w", None, o).unwrap().iter().any(|a| a.starts_with("--request-timeout")));
        let prev = LogOptions { previous: true, ..o };
        let a = logs_args("c", "s", "w", None, prev).unwrap();
        assert!(a.contains(&"--previous".to_string()) && !a.contains(&"-f".to_string()));
        assert!(logs_args("c", "s", "w", Some("Bad Name"), o).is_err());
        assert_eq!(logs_args("c", "s", "w", None, LogOptions { tail: 10_000_000, ..o }).unwrap()[6], "--tail=100000");
        let f = forward_args("c", "shop", &Target_::Service("web".into()), 8080, 80).unwrap();
        assert_eq!(f, ["--context", "c", "port-forward", "service/web", "-n", "shop", "--address", "127.0.0.1", "8080:80"]);
        assert!(forward_args("c", "shop", &Target_::Pod("web".into()), 0, 80).is_err());
        assert!(forward_args("c", "shop", &Target_::Pod("web".into()), 80, 70000).is_err());
        assert!(forward_args("c", "shop", &Target_::Pod("We;b".into()), 80, 80).is_err());
    }

    #[test]
    fn pods_are_read_the_way_kubectl_shows_them() {
        let pods = parse_pods(PODS).unwrap();
        let by = |n: &str| pods.iter().find(|p| p.name == n).unwrap();
        // Sorted by namespace, then name; the entry without a name is skipped.
        assert_eq!(pods.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["batch-1", "done", "api-0", "web-7d9f-abc"]);
        let web = by("web-7d9f-abc");
        assert_eq!((web.status.as_str(), web.ready.as_str(), web.restarts), ("CrashLoopBackOff", "1/2", 4));
        assert_eq!(web.owner.as_deref(), Some("ReplicaSet/web-7d9f"));
        assert_eq!((web.node.as_deref(), web.ip.as_deref()), (Some("node-1"), Some("10.1.2.3")));
        assert_eq!(web.containers, ["app", "sidecar"]);
        assert_eq!(by("batch-1").status, "Terminating");
        assert_eq!(by("api-0").status, "PodInitializing");
        assert_eq!(by("done").status, "Succeeded");
        assert_eq!(by("api-0").ready, "0/1");
    }

    #[test]
    fn bad_answers_are_errors_not_panics() {
        assert!(parse_pods("not json").is_err());
        assert!(parse_pods("{}").is_err());
        assert_eq!(parse_pods(r#"{"items":[]}"#).unwrap(), Vec::new());
        assert_eq!(parse_names("namespace/b\nnamespace/a\n\nnamespace/a\n"), ["a", "b"]);
    }

    fn out(code: i32, stderr: &str) -> Output {
        Output { stdout: String::new(), stderr: stderr.into(), code: Some(code) }
    }

    #[test]
    fn failures_explain_themselves() {
        assert!(matches!(failure(&out(127, "sh: kubectl: not found")), ContainerError::NotInstalled(_)));
        let e = failure(&out(1, "Unable to connect to the server: dial tcp 10.0.0.1:6443: i/o timeout")).to_string();
        assert!(e.contains("cluster isn't answering"), "{e}");
        assert!(failure(&out(1, "Error from server (Forbidden): pods is forbidden")).to_string().contains("isn't allowed"));
        assert!(failure(&out(1, "error: You must be logged in to the server (Unauthorized)")).to_string().contains("expired"));
        let plain = failure(&out(1, "Error from server (NotFound): pods \"x\" not found")).to_string();
        assert_eq!(plain, "Error from server (NotFound): pods \"x\" not found");
    }

    // -- through the real transport, with a stand-in kubectl ---------------------------------------

    #[cfg(unix)]
    struct Collect(Mutex<Vec<LogEvent>>);
    #[cfg(unix)]
    impl LogSink for Collect {
        fn event(&self, e: LogEvent) {
            self.0.lock().unwrap().push(e);
        }
    }
    #[cfg(unix)]
    impl Collect {
        fn text(&self) -> String {
            self.0.lock().unwrap().iter().filter_map(|e| if let LogEvent::Chunk { text } = e { Some(text.as_str()) } else { None }).collect()
        }
        fn ended(&self) -> bool {
            self.0.lock().unwrap().iter().any(|e| matches!(e, LogEvent::End { .. }))
        }
    }

    #[cfg(unix)]
    async fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    /// A `kubectl` that answers like the real one for the calls the app makes.
    #[cfg(unix)]
    fn fake_kubectl(dir: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let pods = PODS.replace('\n', " ");
        let script = format!(
            r#"#!/bin/sh
all=" $* "
case "$all" in
  *" version "*) echo '{{"clientVersion":{{"gitVersion":"v1.30.2"}}}}' ;;
  *" get-contexts "*) printf 'prod\nstaging\n' ;;
  *" current-context "*) echo prod ;;
  *" get pods "*)
    if [ -f "{dir}/down" ]; then echo 'Unable to connect to the server: dial tcp 10.0.0.1:6443: i/o timeout' >&2; exit 1; fi
    echo '{pods}' ;;
  *" get namespaces "*) printf 'namespace/shop\nnamespace/jobs\n' ;;
  *" describe "*) echo "Name: $6" ;;
  *" delete "*) echo "pod \"$3\" deleted"; echo "$*" > "{dir}/deleted" ;;
  *" logs "*)
    echo "line one"; echo "line two"
    case "$all" in *" -f "*) while true; do echo tick; sleep 0.1; done ;; esac ;;
  *" port-forward "*) echo "Forwarding from 127.0.0.1:8080 -> 80"; while true; do sleep 0.1; done ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#,
            dir = dir.display(),
            pods = pods.replace('\'', "'\\''")
        );
        let path = dir.join("kubectl");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn pods_logs_and_forwards_through_a_kubectl() {
        let dir = tempfile::TempDir::new().unwrap();
        fake_kubectl(dir.path());
        // In front of the real PATH, so everything else keeps working.
        let old = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{old}", dir.path().display()));

        let m = Arc::new(KubeManager::new());
        let (id, info) = m.open(None).await.unwrap();
        assert_eq!((info.version.as_str(), info.current.as_deref()), ("v1.30.2", Some("prod")));
        assert_eq!(info.contexts, ["prod", "staging"]);

        let pods = m.pods(id, "prod", &Scope::All).await.unwrap();
        assert_eq!(pods.len(), 4);
        assert_eq!(m.namespaces(id, "prod").await.unwrap(), ["jobs", "shop"]);
        assert!(m.describe(id, "prod", "shop", "web").await.unwrap().contains("Name: web"));

        // Names that aren't names never reach kubectl.
        assert!(matches!(m.pods(id, "prod", &Scope::Namespace("a;b".into())).await, Err(ContainerError::Invalid(_))));
        assert!(matches!(m.pods(id, "$(id)", &Scope::All).await, Err(ContainerError::Invalid(_))));
        assert!(matches!(m.delete_pod(id, "prod", "shop", "x y").await, Err(ContainerError::Invalid(_))));
        assert!(!dir.path().join("deleted").exists());

        m.delete_pod(id, "prod", "shop", "web-7d9f-abc").await.unwrap();
        let deleted = std::fs::read_to_string(dir.path().join("deleted")).unwrap();
        assert!(deleted.contains("delete pod web-7d9f-abc -n shop --wait=false"), "{deleted}");

        // A followed log streams until stopped.
        let logs = Arc::new(Collect(Mutex::new(Vec::new())));
        let o = LogOptions { tail: 100, follow: true, timestamps: false, previous: false };
        let sid = m.start_logs(id, "prod", "shop", "web-7d9f-abc", Some("app"), o, logs.clone()).unwrap();
        wait_for("lines", || logs.text().matches("tick").count() >= 2).await;
        assert!(logs.text().contains("line one\nline two"));
        assert_eq!(m.stream_count(), 1);
        m.stop_stream(sid);
        wait_for("the end", || logs.ended()).await;
        assert_eq!(m.stream_count(), 0);

        // A port-forward says what kubectl says, and stops with its session.
        let fwd = Arc::new(Collect(Mutex::new(Vec::new())));
        m.start_forward(id, "prod", "shop", &Target_::Service("web".into()), 8080, 80, fwd.clone()).unwrap();
        wait_for("the forward", || fwd.text().contains("Forwarding from 127.0.0.1:8080 -> 80")).await;
        m.close(id).await;
        wait_for("the forward to end with its session", || fwd.ended()).await;
        assert_eq!(m.stream_count(), 0);

        // A cluster that doesn't answer is an error with a hint, not a hang.
        let (id2, _) = m.open(None).await.unwrap();
        std::fs::write(dir.path().join("down"), "").unwrap();
        let e = m.pods(id2, "prod", &Scope::All).await.unwrap_err().to_string();
        assert!(e.contains("Unable to connect") && e.contains("cluster isn't answering"), "{e}");
        m.close(id2).await;
        std::env::set_var("PATH", old);
    }

    fn row(kind: Kind, json: &str) -> Vec<Resource> {
        parse_resources(kind, json).unwrap()
    }
    fn detail<'a>(r: &'a Resource, label: &str) -> &'a str {
        r.details.iter().find(|x| x.label == label).map(|x| x.value.as_str()).unwrap_or("<missing>")
    }

    #[test]
    fn other_kinds_are_listed_with_the_columns_that_matter() {
        let deployments = row(
            Kind::Deployments,
            r#"{"items":[{"metadata":{"name":"web","namespace":"shop","creationTimestamp":"2026-10-01T10:00:00Z"},"spec":{"replicas":3},"status":{"readyReplicas":2,"updatedReplicas":3,"availableReplicas":2}},
                         {"metadata":{"name":"idle","namespace":"shop"},"spec":{"replicas":0},"status":{}}]}"#,
        );
        assert_eq!(deployments.iter().map(|r| (r.name.as_str(), r.status.as_str())).collect::<Vec<_>>(), [("idle", "0/0 ready"), ("web", "2/3 ready")]);
        assert_eq!((detail(&deployments[1], "Up to date"), detail(&deployments[1], "Available")), ("3", "2"));

        let services = row(
            Kind::Services,
            r#"{"items":[{"metadata":{"name":"web","namespace":"shop"},"spec":{"type":"LoadBalancer","clusterIP":"10.0.0.5","ports":[{"port":80,"protocol":"TCP"},{"port":443}]},"status":{"loadBalancer":{"ingress":[{"ip":"34.1.2.3"}]}}},
                         {"metadata":{"name":"db","namespace":"shop"},"spec":{"clusterIP":"10.0.0.9","ports":[{"port":5432,"protocol":"TCP"}]}}]}"#,
        );
        assert_eq!((services[1].status.as_str(), detail(&services[1], "External"), detail(&services[1], "Ports")), ("LoadBalancer", "34.1.2.3", "80/TCP, 443/TCP"));
        assert_eq!((services[0].status.as_str(), detail(&services[0], "External")), ("ClusterIP", ""));

        let sets = row(Kind::StatefulSets, r#"{"items":[{"metadata":{"name":"pg","namespace":"d"},"spec":{"replicas":2,"serviceName":"pg-hl"},"status":{"readyReplicas":1}}]}"#);
        assert_eq!((sets[0].status.as_str(), detail(&sets[0], "Service")), ("1/2 ready", "pg-hl"));
        let ds = row(Kind::DaemonSets, r#"{"items":[{"metadata":{"name":"agent","namespace":"kube-system"},"status":{"numberReady":3,"desiredNumberScheduled":4,"numberAvailable":3}}]}"#);
        assert_eq!(ds[0].status, "3/4 ready");
    }

    #[test]
    fn config_maps_show_key_names_never_values() {
        let maps = row(
            Kind::ConfigMaps,
            r#"{"items":[{"metadata":{"name":"app","namespace":"shop"},"data":{"DB_URL":"postgres://u:hunter2@db/x","FLAG":"on"}},
                         {"metadata":{"name":"empty","namespace":"shop"}}]}"#,
        );
        assert_eq!((maps[0].status.as_str(), detail(&maps[0], "Keys")), ("2 keys", "DB_URL, FLAG"));
        assert_eq!(maps[1].status, "0 keys");
        let all = serde_json::to_string(&maps).unwrap();
        assert!(!all.contains("hunter2") && !all.contains("postgres://"), "{all}");
        let many: Vec<String> = (0..12).map(|i| format!("\"K{i:02}\":\"v\"")).collect();
        let big = row(Kind::ConfigMaps, &format!(r#"{{"items":[{{"metadata":{{"name":"b","namespace":"x"}},"data":{{{}}}}}]}}"#, many.join(",")));
        assert!(detail(&big[0], "Keys").ends_with(" …") && big[0].status == "12 keys");
    }

    #[test]
    fn jobs_cron_jobs_ingresses_and_nodes() {
        let jobs = row(
            Kind::Jobs,
            r#"{"items":[{"metadata":{"name":"ok","namespace":"j"},"spec":{"completions":1},"status":{"succeeded":1,"conditions":[{"type":"Complete","status":"True"}]}},
                         {"metadata":{"name":"bad","namespace":"j"},"spec":{},"status":{"failed":3,"conditions":[{"type":"Failed","status":"True"}]}},
                         {"metadata":{"name":"going","namespace":"j"},"spec":{"completions":5},"status":{"succeeded":2}}]}"#,
        );
        let by = |n: &str| jobs.iter().find(|j| j.name == n).unwrap();
        assert_eq!((by("ok").status.as_str(), by("bad").status.as_str(), by("going").status.as_str()), ("Complete", "Failed", "Running"));
        assert_eq!((detail(by("going"), "Completions"), detail(by("bad"), "Failed pods")), ("2/5", "3"));

        let cron = row(Kind::CronJobs, r#"{"items":[{"metadata":{"name":"nightly","namespace":"j"},"spec":{"schedule":"0 2 * * *","suspend":true},"status":{"lastScheduleTime":"2026-10-08T02:00:00Z"}}]}"#);
        assert_eq!((cron[0].status.as_str(), detail(&cron[0], "Schedule"), detail(&cron[0], "Last run")), ("Suspended", "0 2 * * *", "2026-10-08T02:00:00Z"));

        let ing = row(Kind::Ingresses, r#"{"items":[{"metadata":{"name":"web","namespace":"shop"},"spec":{"ingressClassName":"nginx","rules":[{"host":"a.example"},{"host":"b.example"}]}}]}"#);
        assert_eq!((ing[0].status.as_str(), detail(&ing[0], "Hosts")), ("nginx", "a.example, b.example"));

        let nodes = row(
            Kind::Nodes,
            r#"{"items":[{"metadata":{"name":"cp-1","labels":{"node-role.kubernetes.io/control-plane":"","kubernetes.io/os":"linux"}},"status":{"conditions":[{"type":"Ready","status":"True"}],"nodeInfo":{"kubeletVersion":"v1.30.2"},"addresses":[{"type":"InternalIP","address":"10.0.0.1"}]}},
                         {"metadata":{"name":"w-1"},"status":{"conditions":[{"type":"Ready","status":"False"}]}}]}"#,
        );
        assert_eq!((nodes[0].namespace.as_str(), nodes[0].status.as_str(), detail(&nodes[0], "Roles"), detail(&nodes[0], "Version"), detail(&nodes[0], "Internal IP")), ("", "Ready", "control-plane", "v1.30.2", "10.0.0.1"));
        assert_eq!((nodes[1].status.as_str(), detail(&nodes[1], "Roles")), ("NotReady", "<none>"));
    }

    #[test]
    fn events_come_newest_first_with_long_messages_cut() {
        let long = "x".repeat(900);
        let json = format!(
            r#"{{"items":[
              {{"metadata":{{"name":"e1","namespace":"shop"}},"type":"Normal","reason":"Pulled","involvedObject":{{"kind":"Pod","name":"web-1"}},"message":"pulled","count":1,"lastTimestamp":"2026-10-08T10:00:00Z"}},
              {{"metadata":{{"name":"e2","namespace":"shop"}},"type":"Warning","reason":"BackOff","involvedObject":{{"kind":"Pod","name":"web-2"}},"message":"{long}\nsecond line","count":7,"lastTimestamp":"2026-10-08T12:00:00Z"}},
              {{"metadata":{{"name":"e3","namespace":"shop","creationTimestamp":"2026-10-08T11:00:00Z"}},"type":"Normal","reason":"Scheduled","involvedObject":{{"kind":"Pod","name":"web-3"}},"message":"ok"}}
            ]}}"#
        );
        let ev = row(Kind::Events, &json);
        assert_eq!(ev.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["e2", "e3", "e1"]);
        assert_eq!((ev[0].status.as_str(), detail(&ev[0], "Reason"), detail(&ev[0], "Object"), detail(&ev[0], "Count")), ("Warning", "BackOff", "Pod/web-2", "7"));
        assert!(detail(&ev[0], "Message").chars().count() <= MAX_MESSAGE + 1 && detail(&ev[0], "Message").ends_with('…') && !detail(&ev[0], "Message").contains("second"));
        let flood: Vec<String> = (0..700).map(|i| format!(r#"{{"metadata":{{"name":"e{i}"}},"lastTimestamp":"2026-10-08T10:{:02}:{:02}Z"}}"#, i / 60 % 60, i % 60)).collect();
        assert_eq!(row(Kind::Events, &format!(r#"{{"items":[{}]}}"#, flood.join(","))).len(), MAX_EVENTS);
    }

    #[test]
    fn resource_commands_are_built_from_checked_names_and_never_ask_for_secrets() {
        let ns = Scope::Namespace("shop".into());
        assert_eq!(resources_args("prod", &ns, Kind::Deployments).unwrap()[3..], ["get", "deployments", "-n", "shop", "-o", "json"]);
        assert_eq!(resources_args("prod", &Scope::All, Kind::Events).unwrap()[3..], ["get", "events", "-A", "-o", "json"]);
        // A node has no namespace, whatever scope was chosen.
        assert_eq!(resources_args("prod", &ns, Kind::Nodes).unwrap()[3..], ["get", "nodes", "-o", "json"]);
        assert_eq!(describe_resource_args("prod", "shop", Kind::Services, "web").unwrap()[3..], ["describe", "services", "web", "-n", "shop"]);
        assert_eq!(describe_resource_args("prod", "", Kind::Nodes, "cp-1").unwrap()[3..], ["describe", "nodes", "cp-1"]);
        assert!(resources_args("prod", &Scope::Namespace("a;b".into()), Kind::Jobs).is_err());
        assert!(resources_args("$(id)", &Scope::All, Kind::Jobs).is_err());
        assert!(describe_resource_args("prod", "shop", Kind::Services, "x y").is_err());
        // Every kind that can be asked for, by name: Secrets are not among them, and the JSON can't name one.
        let all = [Kind::Deployments, Kind::StatefulSets, Kind::DaemonSets, Kind::Services, Kind::ConfigMaps, Kind::Jobs, Kind::CronJobs, Kind::Ingresses, Kind::Events, Kind::Nodes];
        assert!(all.iter().all(|k| !k.api_name().contains("secret")));
        // The names the window sends (RESOURCE_KINDS in kubedata.ts), in the same order.
        let wire: Vec<String> = all.iter().map(|k| serde_json::to_string(k).unwrap().trim_matches('"').to_string()).collect();
        assert_eq!(wire, ["deployments", "stateful_sets", "daemon_sets", "services", "config_maps", "jobs", "cron_jobs", "ingresses", "events", "nodes"]);
        assert!(serde_json::from_str::<Kind>("\"secrets\"").is_err());
        assert_eq!(serde_json::from_str::<Kind>("\"config_maps\"").unwrap(), Kind::ConfigMaps);
        assert!(parse_resources(Kind::Jobs, "not json").is_err());
        assert!(parse_resources(Kind::Jobs, "{}").is_err());
    }
}
