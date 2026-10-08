# Build (from repo root): docker build -f docker/apache.Dockerfile -t drop-apache:latest .
# Serves /usr/local/apache2/htdocs/tag: mount the drops volume there.

FROM httpd:2.4
# same as ../apache-conf/conf/my-httpd.conf: stock config with the vhosts include enabled
RUN sed -i 's|^#Include conf/extra/httpd-vhosts.conf|Include conf/extra/httpd-vhosts.conf|' conf/httpd.conf \
    && grep -q '^Include conf/extra/httpd-vhosts.conf' conf/httpd.conf
COPY docker/apache/httpd-vhosts.conf conf/extra/httpd-vhosts.conf
