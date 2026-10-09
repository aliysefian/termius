//! Containers.

use super::*;

/// What the window learns about an opened source.
#[derive(Debug, Clone, Serialize)]
pub struct ContainersOpened {
    pub session_id: Uuid,
    pub runtimes: Vec<crate::containers::Runtime>,
}

/// Open this computer (`host_id` empty) or a saved host. The host needs saved
/// credentials: the connection is made without asking.
#[tauri::command]
pub async fn containers_open(state: State<'_, AppState>, host_id: Option<Uuid>) -> ApiResult<ContainersOpened> {
    let via = match host_id {
        Some(id) => Some(resolve_target(&state, id, None)?),
        None => None,
    };
    let (session_id, runtimes) = state.containers.open(via).await?;
    Ok(ContainersOpened { session_id, runtimes })
}

// -- Kubernetes ----------------------------------------------------------------------

#[derive(Serialize)]
pub struct KubeOpened {
    pub session_id: Uuid,
    pub info: crate::kube::KubeInfo,
}

/// Open this computer (`host_id` empty) or a saved host with saved credentials, and read its kubectl contexts.
#[tauri::command]
pub async fn kube_open(state: State<'_, AppState>, host_id: Option<Uuid>) -> ApiResult<KubeOpened> {
    let via = match host_id {
        Some(id) => Some(resolve_target(&state, id, None)?),
        None => None,
    };
    let (session_id, info) = state.kube.open(via).await?;
    Ok(KubeOpened { session_id, info })
}

#[tauri::command]
pub async fn kube_close(state: State<'_, AppState>, session_id: Uuid) -> ApiResult<()> {
    state.kube.close(session_id).await;
    Ok(())
}

#[tauri::command]
pub async fn kube_pods(state: State<'_, AppState>, session_id: Uuid, context: String, scope: crate::kube::Scope) -> ApiResult<Vec<crate::kube::Pod>> {
    Ok(state.kube.pods(session_id, &context, &scope).await?)
}

/// Deployments, services, events and the other read-only kinds (see `kube::Kind`; Secrets are not among them).
#[tauri::command]
pub async fn kube_resources(state: State<'_, AppState>, session_id: Uuid, context: String, scope: crate::kube::Scope, kind: crate::kube::Kind) -> ApiResult<Vec<crate::kube::Resource>> {
    Ok(state.kube.resources(session_id, &context, &scope, kind).await?)
}

#[tauri::command]
pub async fn kube_describe_resource(state: State<'_, AppState>, session_id: Uuid, context: String, namespace: String, kind: crate::kube::Kind, name: String) -> ApiResult<String> {
    Ok(state.kube.describe_resource(session_id, &context, &namespace, kind, &name).await?)
}

#[tauri::command]
pub async fn kube_namespaces(state: State<'_, AppState>, session_id: Uuid, context: String) -> ApiResult<Vec<String>> {
    Ok(state.kube.namespaces(session_id, &context).await?)
}

#[tauri::command]
pub async fn kube_describe(state: State<'_, AppState>, session_id: Uuid, context: String, namespace: String, pod: String) -> ApiResult<String> {
    Ok(state.kube.describe(session_id, &context, &namespace, &pod).await?)
}

#[tauri::command]
pub async fn kube_delete_pod(state: State<'_, AppState>, session_id: Uuid, context: String, namespace: String, pod: String) -> ApiResult<()> {
    Ok(state.kube.delete_pod(session_id, &context, &namespace, &pod).await?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn kube_logs_start(
    state: State<'_, AppState>,
    session_id: Uuid,
    context: String,
    namespace: String,
    pod: String,
    container: Option<String>,
    options: crate::kube::LogOptions,
    on_event: Channel<crate::containers::LogEvent>,
) -> ApiResult<Uuid> {
    let sink: Arc<dyn crate::containers::LogSink> = Arc::new(ChannelLogSink(on_event));
    Ok(state.kube.start_logs(session_id, &context, &namespace, &pod, container.as_deref(), options, sink)?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn kube_forward_start(
    state: State<'_, AppState>,
    session_id: Uuid,
    context: String,
    namespace: String,
    to: crate::kube::Target_,
    local_port: u32,
    remote_port: u32,
    on_event: Channel<crate::containers::LogEvent>,
) -> ApiResult<Uuid> {
    let sink: Arc<dyn crate::containers::LogSink> = Arc::new(ChannelLogSink(on_event));
    Ok(state.kube.start_forward(session_id, &context, &namespace, &to, local_port, remote_port, sink)?)
}

/// Stop a followed log or a port-forward.
#[tauri::command]
pub fn kube_stop(state: State<'_, AppState>, stream_id: Uuid) {
    state.kube.stop_stream(stream_id);
}

#[tauri::command]
pub async fn containers_close(state: State<'_, AppState>, session_id: Uuid) -> ApiResult<()> {
    state.containers.close(session_id).await;
    Ok(())
}

#[tauri::command]
pub async fn containers_list(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    sizes: bool,
) -> ApiResult<crate::containers::Listing> {
    Ok(state.containers.list(session_id, runtime, sizes).await?)
}

#[tauri::command]
pub async fn containers_act(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    action: crate::containers::Action,
    id: String,
) -> ApiResult<()> {
    Ok(state.containers.act(session_id, runtime, action, &id).await?)
}

#[tauri::command]
pub async fn containers_inspect(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    id: String,
) -> ApiResult<String> {
    Ok(state.containers.inspect(session_id, runtime, &id).await?)
}

pub(super) struct ChannelLogSink(Channel<crate::containers::LogEvent>);
impl crate::containers::LogSink for ChannelLogSink {
    fn event(&self, e: crate::containers::LogEvent) {
        let _ = self.0.send(e);
    }
}

/// Start reading a container's log. Events arrive on `on_event`; the returned
/// id stops it.
#[tauri::command]
pub fn containers_logs_start(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    id: String,
    options: crate::containers::LogOptions,
    on_event: Channel<crate::containers::LogEvent>,
) -> ApiResult<Uuid> {
    let sink: Arc<dyn crate::containers::LogSink> = Arc::new(ChannelLogSink(on_event));
    Ok(state.containers.start_logs(session_id, runtime, &id, options, sink)?)
}

/// Volumes and networks of a Docker source, with the containers using each.
/// CPU, memory, network and disk use of the running containers, one reading.
#[tauri::command]
pub async fn containers_stats(state: State<'_, AppState>, session_id: Uuid, runtime: crate::containers::Runtime) -> ApiResult<Vec<crate::containers::parse::Stat>> {
    Ok(state.containers.stats(session_id, runtime).await?)
}

#[tauri::command]
pub async fn containers_resources(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    sizes: bool,
) -> ApiResult<crate::containers::Resources> {
    Ok(state.containers.resources(session_id, runtime, sizes).await?)
}

#[tauri::command]
pub async fn containers_remove(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    kind: crate::containers::ResourceKind,
    id: String,
    force: bool,
) -> ApiResult<()> {
    Ok(state.containers.remove(session_id, runtime, kind, &id, force).await?)
}

/// What a prune would remove. Removes nothing.
#[tauri::command]
pub async fn containers_prune_preview(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    kind: crate::containers::prune::PruneKind,
) -> ApiResult<Vec<crate::containers::prune::PruneItem>> {
    Ok(state.containers.prune_preview(session_id, runtime, kind).await?)
}

/// Remove exactly the given items, each re-checked first.
#[tauri::command]
pub async fn containers_prune_run(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    kind: crate::containers::prune::PruneKind,
    ids: Vec<String>,
) -> ApiResult<Vec<crate::containers::prune::PruneResult>> {
    Ok(state.containers.prune_run(session_id, runtime, kind, ids).await?)
}

/// Pull an image; progress arrives on `on_event`, and the id stops it.
#[tauri::command]
pub fn containers_pull(
    state: State<'_, AppState>,
    session_id: Uuid,
    runtime: crate::containers::Runtime,
    reference: String,
    on_event: Channel<crate::containers::LogEvent>,
) -> ApiResult<Uuid> {
    let sink: Arc<dyn crate::containers::LogSink> = Arc::new(ChannelLogSink(on_event));
    Ok(state.containers.start_pull(session_id, runtime, &reference, sink)?)
}

#[tauri::command]
pub async fn containers_compose(
    state: State<'_, AppState>,
    session_id: Uuid,
    project: String,
    verb: crate::containers::ComposeVerb,
) -> ApiResult<()> {
    Ok(state.containers.compose(session_id, &project, verb).await?)
}

#[tauri::command]
pub fn containers_logs_stop(state: State<'_, AppState>, stream_id: Uuid) {
    state.containers.stop_stream(stream_id);
}
