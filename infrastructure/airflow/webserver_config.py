"""
It uses GitLab's OIDC `/oauth/userinfo` endpoint to read the user's GitLab subgroup membership (`workshop/admins`, `workshop/maintainers`, `workshop/users`, `workshop/viewers`) and dynamically maps `workshop/users` to the `User` role in `airflow-dev` and `Viewer` in `airflow-val` and `airflow-prd`.
"""

import os
import logging
from flask_appbuilder.security.manager import AUTH_OAUTH
from airflow.providers.fab.auth_manager.security_manager.override import FabAirflowSecurityManagerOverride

basedir = os.path.abspath(os.path.dirname(__file__))
log = logging.getLogger(__name__)

DEPLOY_TARGET = os.getenv("DBT_DEPLOY_TARGET", "dev")
GITLAB_INTERNAL_URL = "http://gitlab.local"
GITLAB_BROWSER_URL = "http://gitlab.local"

AUTH_TYPE = AUTH_OAUTH
AUTH_USER_REGISTRATION = True
AUTH_USER_REGISTRATION_ROLE = "Public"
AUTH_ROLES_SYNC_AT_LOGIN = True

# Map GitLab subgroups to Airflow FAB Roles
# Developers ('workshop/users') can trigger DAGs in dev, but are read-only Viewers in val and prd
AUTH_ROLES_MAPPING = {
    "workshop/admins": ["Admin"],
    "workshop/maintainers": ["Op"],
    "workshop/users": ["User"] if DEPLOY_TARGET == "dev" else ["Viewer"],
    "workshop/viewers": ["Viewer"],
}

OAUTH_PROVIDERS = [
    {
        "name": "gitlab",
        "icon": "fa-gitlab",
        "token_key": "access_token",
        "remote_app": {
            "client_id": os.getenv("GITLAB_CLIENT_ID"),
            "client_secret": os.getenv("GITLAB_CLIENT_SECRET"),
            "api_base_url": f"{GITLAB_INTERNAL_URL}/",
            "client_kwargs": {"scope": "read_user openid profile email"},
            "request_token_url": None,
            "access_token_url": f"{GITLAB_INTERNAL_URL}/oauth/token",
            "authorize_url": f"{GITLAB_BROWSER_URL}/oauth/authorize",
            "jwks_uri": f"{GITLAB_INTERNAL_URL}/oauth/discovery/keys",
        },
    }
]


class GitLabSubgroupSecurityManager(FabAirflowSecurityManagerOverride):
    def get_oauth_user_info(self, provider, response):
        if provider == "gitlab":
            me = self.appbuilder.sm.oauth_remotes[provider].get("oauth/userinfo")
            data = me.json()
            groups = data.get("groups", [])
            log.info("GitLab OAuth login for %s with groups: %s", data.get("preferred_username"), groups)
            return {
                "username": data.get("preferred_username"),
                "email": data.get("email"),
                "first_name": data.get("name", "").split(" ")[0],
                "last_name": " ".join(data.get("name", "").split(" ")[1:]) or "Workshop",
                "role_keys": groups,
            }
        return {}


SECURITY_MANAGER_CLASS = GitLabSubgroupSecurityManager
