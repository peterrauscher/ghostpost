# Nightly encrypted Postgres backup to R2. Same Postgres major as the server.
FROM postgres:17-bookworm@sha256:4f736ae292687621d4dbe0d499ffd024a36bd2ee7d8ca6f2ccd4c800f047b394
RUN apt-get update \
    && apt-get install -y --no-install-recommends age curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -o postgres -g postgres -m 0700 /var/spool/ghostpost-backup
COPY backup.sh /usr/local/bin/ghostpost-backup
RUN chmod 0555 /usr/local/bin/ghostpost-backup
USER postgres
ENTRYPOINT ["/usr/local/bin/ghostpost-backup"]
CMD ["loop"]
