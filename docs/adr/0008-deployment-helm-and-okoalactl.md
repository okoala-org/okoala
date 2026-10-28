# 0008. Deployment: Helm only, with the `okoalactl` installer

- Status: Accepted, 2026-10-22

## Context

OKoala runs several roles, PostgreSQL, S3-compatible storage, Ory and a homeserver per
chat-enabled tenant. Restarts, rolling upgrades, volumes, certificates, leader election
and backups are what Kubernetes, cert-manager and CloudNativePG already do. Supporting a
second deployment method (such as Docker Compose) means a second backup path, a second
certificate path and a second set of bugs. The chat provisioner needs a Kubernetes API on
every installation. Small installations still need a one-command install.

## Decision

- **The Helm chart is the only supported deployment**, on any conformant Kubernetes.
- **A single server is a one-machine k3s cluster running the same chart.** Larger
  installations are multi-machine k3s clusters or any Kubernetes, managed services included.
- Terms: a *machine* is one Linux host in the cluster; a *role* is one duty of the
  `okoala` binary; a *profile* is how the chart groups roles into pods (`split`: one
  Deployment per role, two or more replicas; `single`: serving roles in one pod).
- **cert-manager everywhere.** The server has no built-in ACME. Tenant-domain
  certificates and Ingress hosts are created by the `provisioner` role once the domain's
  DNS is verified.
- **CloudNativePG and barman-cloud everywhere** for PostgreSQL and its backups, or an
  external PostgreSQL. Blobs go to S3, in-cluster SeaweedFS on a single machine.
- **Ory's public endpoints go through the Ingress**; their admin ports stay internal.
- **PROXY protocol v2** on SMTP, submission and IMAP, accepted only from configured
  networks; `externalTrafficPolicy: Local` where a load balancer cannot send it.
- **`okoalactl`** is a separate installer binary, in Rust in the same workspace,
  depending only on `okoala-config`, `okoala-secret` and `okoala-core`. Commands:
  `preflight` (host checks), `install --single`, `install --cluster-init`, `join`,
  `upgrade`, `backup`, `restore-drill`, `restore`, `rotate-keys`, and `dev up` for
  development clusters (k3d or kind on macOS).
- `okoalactl` downloads pinned k3s and Helm releases and verifies them against checksums
  compiled into it. It generates every secret once, as a Kubernetes Secret, never at
  chart render time. `install --single` requires an off-host S3 backup target unless the
  operator passes an explicit "no backups" flag.
- Every command that changes a release first runs `okoala serve --check` on the rendered
  configuration and refuses to start an empty database next to existing data.

## Consequences

- **Least privilege**: the `okoala` server embeds no Docker, Helm or k3s and holds no
  deployment powers. Serving pods have no Kubernetes token, no owner database credential
  and no access to backups; the provisioner pod is the one exception, limited by RBAC
  (invariant I14).
- One backup design, one certificate path and one upgrade path for every installation.
- Production hosts must be Linux. Small operators need some Kubernetes concepts;
  `okoalactl` wraps the routine operations and runbooks give exact commands.
- k3s itself costs memory and CPU on small machines; the minimum supported size is
  measured with every component on.
- A `--single` install stays one machine; converting it is a backup and restore.

## Alternatives considered

- **Docker Compose as a second target.** Rejected: a second backup, certificate and
  upgrade path, and no Kubernetes API for the provisioner.
- **Embedding Docker or Helm in the server so it installs itself.** Rejected:
  - driving Docker needs the Docker socket, which is root on the host; an Internet-facing
    mail server holding it turns remote code execution into host compromise;
  - a home-grown supervisor would be a second, less tested orchestrator;
  - Helm, k3s and the Docker client are Go, which means FFI or shelling out from the
    server and a second toolchain;
  - the serving image would carry installers, downloaders and cluster credentials it
    never needs.
- **Built-in ACME in the server.** Rejected with Compose: cert-manager is present on
  every installation.
