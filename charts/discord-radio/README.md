# discord-radio chart

`secret.name` names an existing Secret. The chart reads these keys from it, and each key name equals the environment variable name.

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

```sh
kubectl create secret generic discord-radio-secrets --from-literal=DISCORD_TOKEN=<token>
```
