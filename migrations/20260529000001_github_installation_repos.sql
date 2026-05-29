-- Repositories reachable by each GitHub App installation. Populated by the
-- pull-based sync (truth source) and by installation webhook events (fast
-- path). Used to resolve which installation can mint a clone token for a repo.
CREATE TABLE github_installation_repos (
    installation_id  INTEGER NOT NULL
                     REFERENCES github_installations(id) ON DELETE CASCADE,
    repo_full_name   TEXT NOT NULL,         -- 'owner/name'
    PRIMARY KEY (installation_id, repo_full_name)
);

CREATE INDEX github_installation_repos_repo_idx
    ON github_installation_repos(repo_full_name);
