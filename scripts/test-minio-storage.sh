#!/usr/bin/env bash
# Run the opt-in S3 storage contract and server-side recovery proofs against a
# disposable local MinIO bucket. CI calls this from its separate, non-required
# S3 storage workflow; the regular CI merge gate does not depend on MinIO.
set -euo pipefail

minio_image="minio/minio@sha256:c7175077d39a8cc10c9fd611cdcc68b6a5b365793e9ac6f4198ffff1ef0fe555"
mc_image="minio/mc@sha256:a7fe349ef4bd8521fb8497f55c6042871b2ae640607cf99d9bede5e9bdf11727"
container_name="ugoite-minio-test-$$"
network_name="ugoite-minio-test-$$"
access_key="ugoite-test-access"
secret_key="$(openssl rand -hex 24)"
bucket="${UGOITE_S3_TEST_BUCKET:-ugoite-test}"

cleanup() {
  docker rm --force "$container_name" >/dev/null 2>&1 || true
  docker network rm "$network_name" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

docker network create "$network_name" >/dev/null
docker run --detach --rm --name "$container_name" \
  --network "$network_name" --network-alias minio \
  --publish 127.0.0.1::9000 \
  --env "MINIO_ROOT_USER=$access_key" \
  --env "MINIO_ROOT_PASSWORD=$secret_key" \
  "$minio_image" server /data --address :9000 >/dev/null

port_mapping="$(docker port "$container_name" 9000/tcp | head -n 1)"
endpoint="http://${port_mapping}"
if [[ -z "$port_mapping" ]]; then
  echo "MinIO did not publish its S3 endpoint" >&2
  exit 1
fi

ready=0
for _ in $(seq 1 45); do
  if curl --silent --fail --connect-timeout 2 --max-time 3 \
    "$endpoint/minio/health/ready" >/dev/null; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "$ready" != 1 ]]; then
  echo "MinIO did not become ready" >&2
  exit 1
fi

docker run --rm --network "$network_name" \
  --entrypoint /bin/sh \
  --env "MINIO_ROOT_USER=$access_key" \
  --env "MINIO_ROOT_PASSWORD=$secret_key" \
  --env "MINIO_BUCKET=$bucket" \
  "$mc_image" -ec \
  'mc alias set ci http://minio:9000 "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD" >/dev/null && mc mb --ignore-existing "ci/$MINIO_BUCKET" >/dev/null'

unset AWS_SESSION_TOKEN AWS_PROFILE AWS_WEB_IDENTITY_TOKEN_FILE AWS_ROLE_ARN
export AWS_ACCESS_KEY_ID="$access_key"
export AWS_SECRET_ACCESS_KEY="$secret_key"
export AWS_REGION="${AWS_REGION:-us-east-1}"
export AWS_EC2_METADATA_DISABLED=true
export UGOITE_S3_TEST_ENDPOINT="$endpoint"
export UGOITE_S3_TEST_BUCKET="$bucket"
export UGOITE_S3_TEST_REQUIRED=1

cargo test -p ugoite-storage --test s3_contract --locked
cargo test -p ugoite-iceberg --test s3_recovery --locked
