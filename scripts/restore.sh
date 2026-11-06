#!/bin/sh

set -eou pipefail

CLICKHOUSE_BACKUP_GCS_BUCKET="muzanci-staging-observability-backup"
CLICKHOUSE_BACKUP_DISK_PATH="/var/lib/clickhouse/backup"
CLICKHOUSE_DATABASE_NAME="muzanci_observability"
BACKUP_NAME="test0_backup"

gcloud storage cp "gs://${CLICKHOUSE_BACKUP_GCS_BUCKET}/${BACKUP_NAME}.tar.zst" "${CLICKHOUSE_BACKUP_DISK_PATH}/"

tar --extract \
    --file "${CLICKHOUSE_BACKUP_DISK_PATH}/${BACKUP_NAME}.tar.zst" \
    --directory "${CLICKHOUSE_BACKUP_DISK_PATH}/"

rm "${CLICKHOUSE_BACKUP_DISK_PATH}/${BACKUP_NAME}.tar.zst"

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
