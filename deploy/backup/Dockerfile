# Hourly encrypted off-site backups of the database and server secrets.
FROM alpine:3.22
RUN apk add --no-cache sqlite restic openssh-client curl tzdata ca-certificates
COPY backup.sh entrypoint.sh /usr/local/bin/
RUN chmod 0755 /usr/local/bin/backup.sh /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
