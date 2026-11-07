# discord-radio chart

The chart deploys the bot as a single-replica Deployment with a Service. It also creates:

- A PersistentVolumeClaim for the bot state, mounted at `/data/state`. Set `persistence.existingClaim` to use your own claim instead.
- An emptyDir cache volume mounted at `/data/cache`, capped by `cache.sizeLimit`.
- A content volume mounted at `/data/content`, only when `content.existingClaim` is set. `content.subPath` mounts one folder of the claim.
- An HTTPRoute, only when `httpRoute.enabled` is true. `httpRoute.parentRef.name` is then required.

## Image

The image comes from `image.repository`. `image.tag` defaults to the chart's `appVersion`, and `image.digest` pins the image when set.

## Secret

`secret.name` is required and names an existing Secret. The chart reads these keys from it, and each key name equals the environment variable name.

| Key | Required | Purpose |
| --- | --- | --- |
| `DISCORD_TOKEN` | yes | Discord bot token |
| `BUCKET_ENDPOINT` | no | S3-compatible object storage endpoint |
| `BUCKET_ACCESS_KEY_ID` | no | Object storage access key ID |
| `BUCKET_SECRET_ACCESS_KEY` | no | Object storage secret access key |
| `BUCKET_NAME` | no | Object storage bucket name |
| `GRAFANA_METRICS_URL` | no | Grafana metrics endpoint |
| `GRAFANA_API_KEY` | no | Grafana API key |

Object storage is used only when all four `BUCKET_*` keys are present. Metrics are sent only when both `GRAFANA_*` keys are present.

## Settings

- `web.port` is the port the bot listens on. `web.corsAllowedOrigins` lists the origins allowed by CORS.
- `cache.ttlSecs`, `cache.maxBytes` and `cache.sweepIntervalSecs` set how long cached files live, how large the cache may grow, and how often expired files are removed.
- `persistence.size`, `persistence.storageClass` and `persistence.accessModes` configure the state claim.
- `httpRoute.parentRef` (`name`, `namespace`, `listener`) selects the gateway, and `httpRoute.hostnames` sets the hostnames.
- `resources`, `nodeSelector`, `tolerations`, `affinity` and `podAnnotations` apply to the pod.
