#!/bin/sh

set -eou pipefail

# Should run as user 'clickhouse'

CLICKHOUSE_BACKUP_DISK_NAME="local_backup"
CLICKHOUSE_BACKUP_DISK_PATH="/var/db/clickhouse/local_backup/"
CLICKHOUSE_DATABASE_NAME="muzanci_observability"
BACKUP_NAME="test0_backup"
CLICKHOUSE_BACKUP_GCS_BUCKET="muzanci-staging-observability-backup"

clickhouse-client << EOF
BACKUP DATABASE
  "${CLICKHOUSE_DATABASE_NAME}"
TO
  Disk(
    '${CLICKHOUSE_BACKUP_DISK_NAME}',
    '${BACKUP_NAME}'
)
SETTINGS compression_method = 'zstd';
EOF

tar --create \
    --zstd \
    --file="${CLICKHOUSE_BACKUP_DISK_PATH}/${BACKUP_NAME}.tar.zst" \
    --directory="${CLICKHOUSE_BACKUP_DISK_PATH}" \
    "${BACKUP_NAME}"

gcloud storage cp "${CLICKHOUSE_BACKUP_DISK_PATH}/${BACKUP_NAME}.tar.zst" "gs://${CLICKHOUSE_BACKUP_GCS_BUCKET}/"

rm "${CLICKHOUSE_BACKUP_DISK_PATH}/${BACKUP_NAME}.tar.zst"

rm -r "${CLICKHOUSE_BACKUP_DISK_PATH:?}/${BACKUP_NAME}"
