CREATE DATABASE IF NOT EXISTS "muzanci_observability";

CREATE TABLE "muzanci_observability"."event" (
    "timestamp" DateTime64(3),
    "level" LowCardinality(String),
    "target" String,
    "message" String,
    "error_span_trace" String,
    "attr" JSON
) ENGINE = MergeTree()
ORDER BY ("timestamp", "level");

CREATE USER "muzanci_observability_writer" IDENTIFIED WITH sha256_password BY 'muzanci_observability';

GRANT INSERT, SELECT ON "muzanci_observability"."event" TO "muzanci_observability_writer";
