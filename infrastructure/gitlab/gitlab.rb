# Omnibus configuration of the workshop GitLab (image gitlab/gitlab-ce, pinned in compose/docker-compose.yml).
# Mounted as a compose config at /omnibus_config.rb and loaded with GITLAB_OMNIBUS_CONFIG="from_file('/omnibus_config.rb')":
# GitLab applies it on every container start (reconfigure), on top of the defaults in the gitlab_config volume.
# Changing it: edit, then `make up` (recreates workshop-gitlab).

# Browser and in-network name: the container's hostname and its network alias on workshop_net
external_url 'http://gitlab.local'

# Laptop sizing: one Puma process, a few threads; the compose service caps the container at 4 GB
puma['worker_processes'] = 0
puma['min_threads'] = 1
puma['max_threads'] = 4
sidekiq['concurrency'] = 4
postgresql['shared_buffers'] = "256MB"

# Not needed for the workshop: monitoring, KAS and the container registry
prometheus_monitoring['enable'] = false
alertmanager['enable'] = false
node_exporter['enable'] = false
redis_exporter['enable'] = false
postgres_exporter['enable'] = false
gitlab_exporter['enable'] = false
gitlab_kas['enable'] = false
registry['enable'] = false
