//! What is unused, and so what a prune would remove. Pure functions.
//!
//! The runtime's own `prune` commands are never run. A preview is computed
//! from the lists, shown to the person, and "remove" then deletes exactly
//! those items one by one, each re-checked first. What you saw is what goes.

use serde::{Deserialize, Serialize};

use super::parse::{Container, Image, Network, Volume};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PruneKind {
    /// `all`: every image no container uses. Otherwise only untagged ones.
    Images { all: bool },
    Volumes,
    Networks,
}

/// One thing a prune would remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PruneItem {
    /// What is handed to the remove command: `repo:tag`, an image ID, a name.
    pub id: String,
    pub label: String,
    pub detail: String,
    /// The Compose project it belongs to, so a person sees what they are about to break up.
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PruneResult {
    pub id: String,
    pub ok: bool,
    pub error: Option<String>,
}

const COMPOSE_PROJECT: &str = "com.docker.compose.project";

/// Whether any container (running or not) uses the image. The runtime's own
/// count wins when it gives one. Without a count (older versions print N/A),
/// a container counts if its image field could mean this image: when in
/// doubt it is "in use", so a prune errs towards keeping.
pub fn image_in_use(image: &Image, containers: &[Container]) -> bool {
    if let Some(n) = image.containers {
        return n > 0;
    }
    let tagged = format!("{}:{}", image.repository, image.tag);
    containers.iter().any(|c| {
        let r = c.image.as_str();
        r == tagged
            || (image.tag == "latest" && r == image.repository)
            || (!r.is_empty() && !r.contains([':', '/']) && (image.id.starts_with(r) || r.starts_with(&image.id)))
    })
}

/// How to name an image for removal: its tag, or its ID when it has none.
pub fn image_ref(image: &Image) -> String {
    if image.repository == "<none>" || image.repository.is_empty() || image.tag == "<none>" || image.tag.is_empty() {
        image.id.clone()
    } else {
        format!("{}:{}", image.repository, image.tag)
    }
}

pub fn unused_images(images: &[Image], containers: &[Container], all: bool) -> Vec<PruneItem> {
    let mut out: Vec<PruneItem> = images
        .iter()
        .filter(|i| !image_in_use(i, containers))
        .filter(|i| all || image_ref(i) == i.id)
        .map(|i| {
            let id = image_ref(i);
            let untagged = id == i.id;
            PruneItem {
                label: if untagged { format!("<untagged> {}", &i.id[..i.id.len().min(12)]) } else { id.clone() },
                detail: if i.size_text.is_empty() { String::new() } else { i.size_text.clone() },
                id,
                project: None,
            }
        })
        .collect();
    out.sort_by_key(|i| i.label.to_lowercase());
    out
}

/// Records which containers mount each volume, by name.
pub fn attach_volume_usage(volumes: &mut [Volume], containers: &[Container]) {
    for v in volumes {
        v.used_by = containers.iter().filter(|c| c.mounts.contains(&v.name)).map(|c| c.name.clone()).collect();
    }
}

/// Records which containers are attached to each network, by name.
pub fn attach_network_usage(networks: &mut [Network], containers: &[Container]) {
    for n in networks {
        n.used_by = containers.iter().filter(|c| c.networks.contains(&n.name)).map(|c| c.name.clone()).collect();
    }
}

pub fn unused_volumes(volumes: &[Volume]) -> Vec<PruneItem> {
    let mut out: Vec<PruneItem> = volumes
        .iter()
        .filter(|v| v.used_by.is_empty())
        .map(|v| PruneItem {
            id: v.name.clone(),
            label: v.name.clone(),
            detail: v.size.clone().unwrap_or_default(),
            project: v.labels.get(COMPOSE_PROJECT).cloned(),
        })
        .collect();
    out.sort_by_key(|i| i.label.to_lowercase());
    out
}

pub fn unused_networks(networks: &[Network]) -> Vec<PruneItem> {
    let mut out: Vec<PruneItem> = networks
        .iter()
        .filter(|n| !n.predefined && n.used_by.is_empty())
        .map(|n| PruneItem {
            id: n.name.clone(),
            label: n.name.clone(),
            detail: n.driver.clone(),
            project: n.labels.get(COMPOSE_PROJECT).cloned(),
        })
        .collect();
    out.sort_by_key(|i| i.label.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::parse::{parse_containers, parse_networks, parse_volumes};

    fn image(repo: &str, tag: &str, id: &str, containers: Option<u32>) -> Image {
        Image { id: id.into(), repository: repo.into(), tag: tag.into(), size_text: "10MB".into(), size_bytes: Some(10_000_000), created: None, containers }
    }

    fn container(name: &str, image: &str, mounts: &[&str], networks: &[&str]) -> Container {
        let json = serde_json::json!({
            "ID": format!("{name}{}", "0".repeat(20)), "Names": name, "Image": image, "State": "running",
            "Mounts": mounts.join(","), "Networks": networks.join(","),
        });
        parse_containers(&json.to_string()).unwrap().remove(0)
    }

    #[test]
    fn the_runtimes_own_count_decides_when_it_gives_one() {
        let cs = vec![container("a", "nginx:1", &[], &[])];
        assert!(image_in_use(&image("nginx", "1", "aaa", Some(1)), &cs));
        // Even if a container's text names it, a zero count from the runtime stands.
        assert!(!image_in_use(&image("nginx", "1", "aaa", Some(0)), &cs));
    }

    #[test]
    fn without_a_count_anything_that_could_match_counts_as_used() {
        let cs = vec![container("a", "nginx:1", &[], &[]), container("b", "redis", &[], &[]), container("c", "abc123", &[], &[]), container("d", "reg:5000/team/app:v2", &[], &[])];
        assert!(image_in_use(&image("nginx", "1", "x", None), &cs), "exact repo:tag");
        assert!(image_in_use(&image("redis", "latest", "x", None), &cs), "a bare name means :latest");
        assert!(!image_in_use(&image("redis", "7", "x", None), &cs), "but not another tag");
        assert!(image_in_use(&image("other", "v", "abc123def456", None), &cs), "a container created from an ID");
        assert!(image_in_use(&image("reg:5000/team/app", "v2", "x", None), &cs), "a registry port is not a tag");
        assert!(!image_in_use(&image("unused", "1", "ffffff", None), &cs));
    }

    #[test]
    fn untagged_images_are_removed_by_id_and_tagged_by_name() {
        assert_eq!(image_ref(&image("nginx", "1.27", "abc", None)), "nginx:1.27");
        assert_eq!(image_ref(&image("<none>", "<none>", "abc", None)), "abc");
        assert_eq!(image_ref(&image("", "", "abc", None)), "abc");
    }

    #[test]
    fn image_prune_modes() {
        let images = vec![
            image("nginx", "1", "n1n1n1n1n1n1n1", Some(1)),
            image("old", "1", "o1o1o1o1o1o1o1", Some(0)),
            image("<none>", "<none>", "d1d1d1d1d1d1d1d1", Some(0)),
            image("<none>", "<none>", "d2d2d2d2d2d2d2d2", Some(3)),
        ];
        let dangling: Vec<_> = unused_images(&images, &[], false).iter().map(|i| i.id.clone()).collect();
        assert_eq!(dangling, vec!["d1d1d1d1d1d1d1d1"], "only untagged, and only if nothing uses it");
        let all: Vec<_> = unused_images(&images, &[], true).iter().map(|i| i.id.clone()).collect();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&"old:1".to_string()) && !all.contains(&"nginx:1".to_string()));
        let item = unused_images(&images, &[], true).into_iter().find(|i| i.id == "old:1").unwrap();
        assert_eq!((item.label.as_str(), item.detail.as_str()), ("old:1", "10MB"));
        let untagged = unused_images(&images, &[], false).remove(0);
        assert_eq!(untagged.label, "<untagged> d1d1d1d1d1d1");
    }

    #[test]
    fn volumes_in_use_are_never_candidates() {
        let mut vols = parse_volumes(include_str!("fixtures/docker_volumes.ndjson")).unwrap();
        let cs = parse_containers(include_str!("fixtures/docker_ps_c1.ndjson")).unwrap();
        attach_volume_usage(&mut vols, &cs);
        let used = |n: &str| vols.iter().find(|v| v.name == n).unwrap().used_by.clone();
        assert_eq!(used("sshvault-c1-vol"), vec!["sshvault-c1-box"]);
        assert_eq!(used("sshvault-c1-proj_data"), vec!["sshvault-c1-proj-cache-1"]);
        assert!(unused_volumes(&vols).is_empty(), "both are mounted");
        // With the containers gone, both are unused, and the compose one says which project it belongs to.
        attach_volume_usage(&mut vols, &[]);
        let items = unused_volumes(&vols);
        assert_eq!(items.len(), 2);
        let proj = items.iter().find(|i| i.id == "sshvault-c1-proj_data").unwrap();
        assert_eq!(proj.project.as_deref(), Some("sshvault-c1-proj"));
        assert!(items.iter().find(|i| i.id == "sshvault-c1-vol").unwrap().project.is_none());
    }

    #[test]
    fn a_bind_mount_path_is_not_a_volume_name() {
        // `docker ps` lists bind-mount sources next to volume names; only a name match counts.
        let cs = vec![container("a", "x", &["/tmp", "/var/lib/data", "real-vol"], &[])];
        let mut vols = parse_volumes("{\"Name\":\"tmp\"}\n{\"Name\":\"data\"}\n{\"Name\":\"real-vol\"}").unwrap();
        attach_volume_usage(&mut vols, &cs);
        let used: Vec<_> = vols.iter().map(|v| (v.name.as_str(), v.used_by.len())).collect();
        assert_eq!(used, vec![("tmp", 0), ("data", 0), ("real-vol", 1)]);
    }

    #[test]
    fn predefined_and_attached_networks_are_never_candidates() {
        let mut nets = parse_networks(include_str!("fixtures/docker_networks.ndjson")).unwrap();
        let cs = parse_containers(include_str!("fixtures/docker_ps_c1.ndjson")).unwrap();
        attach_network_usage(&mut nets, &cs);
        let used = |n: &str| nets.iter().find(|x| x.name == n).map(|x| x.used_by.clone()).unwrap_or_default();
        assert_eq!(used("sshvault-c1-net"), vec!["sshvault-c1-box"]);
        assert!(unused_networks(&nets).iter().all(|i| !["bridge", "host", "none"].contains(&i.id.as_str())));
        assert!(unused_networks(&nets).is_empty(), "both real networks have containers attached");
        attach_network_usage(&mut nets, &[]);
        let items = unused_networks(&nets);
        assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["sshvault-c1-net", "sshvault-c1-proj_default"], "bridge, host and none stay out");
        assert_eq!(items[0].detail, "bridge");
        assert_eq!(items[0].project, None);
    }

    #[test]
    fn items_are_sorted_for_reading() {
        let images = vec![image("zeta", "1", "z", Some(0)), image("Alpha", "1", "a", Some(0))];
        assert_eq!(unused_images(&images, &[], true).iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["Alpha:1", "zeta:1"]);
    }
}
