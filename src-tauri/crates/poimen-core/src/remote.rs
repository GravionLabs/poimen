//! The hosting side of a repository's `origin` URL: a GitHub owner or an Azure DevOps
//! organization and project. Pure parsing; reading the URL is `git::origin_url`.

/// A parsed remote. Compare with [`Remote::same`] (owner, organization and project are
/// case-insensitive).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "provider", rename_all = "kebab-case")]
pub enum Remote {
    #[serde(rename = "github")]
    GitHub { owner: String },
    #[serde(rename = "azure-devops")]
    AzureDevOps {
        organization: String,
        project: String,
    },
}

impl Remote {
    /// Same provider and same names, ignoring case.
    pub fn same(&self, other: &Remote) -> bool {
        match (self, other) {
            (Remote::GitHub { owner: a }, Remote::GitHub { owner: b }) => a.eq_ignore_ascii_case(b),
            (
                Remote::AzureDevOps {
                    organization: oa,
                    project: pa,
                },
                Remote::AzureDevOps {
                    organization: ob,
                    project: pb,
                },
            ) => oa.eq_ignore_ascii_case(ob) && pa.eq_ignore_ascii_case(pb),
            _ => false,
        }
    }
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(h) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(h, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Splits a URL (`https://`, `ssh://`, scp-like `user@host:path`) into lowercase host and path.
fn split(url: &str) -> Option<(String, String)> {
    let (authority, path) = if let Some((_, rest)) = url.split_once("://") {
        rest.split_once('/')?
    } else {
        // scp-like: the colon comes before any slash
        let (host, path) = url.split_once(':')?;
        if host.contains('/') {
            return None;
        }
        (host, path)
    };
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?;
    Some((host.to_ascii_lowercase(), path.to_owned()))
}

/// Parses an `origin` URL; `None` for other hosts and shapes without an owner or project.
pub fn parse(url: &str) -> Option<Remote> {
    let url = url.trim();
    let (host, path) = split(url)?;
    let path = path.split(['?', '#']).next().unwrap_or("");
    let seg: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(decode)
        .collect();
    let seg: Vec<&str> = seg.iter().map(String::as_str).collect();
    match host.as_str() {
        "github.com" | "ssh.github.com" => match seg.as_slice() {
            [owner, _repo, ..] => Some(Remote::GitHub {
                owner: (*owner).to_owned(),
            }),
            _ => None,
        },
        "dev.azure.com" => match seg.as_slice() {
            // https://dev.azure.com/<org>/<project>/_git/<repo>
            [org, project, "_git", ..] => azure(org, project),
            // a repository named like its project: <org>/_git/<repo>
            [org, "_git", repo, ..] => azure(org, repo),
            _ => None,
        },
        "ssh.dev.azure.com" | "vs-ssh.visualstudio.com" => match seg.as_slice() {
            ["v3", org, project, _repo, ..] => azure(org, project),
            _ => None,
        },
        h => {
            let org = h.strip_suffix(".visualstudio.com")?;
            match seg.as_slice() {
                [project, "_git", ..] => azure(org, project),
                [_collection, project, "_git", ..] => azure(org, project),
                _ => None,
            }
        }
    }
}

fn azure(org: &str, project: &str) -> Option<Remote> {
    (!org.is_empty() && !project.is_empty() && project != "_git").then(|| Remote::AzureDevOps {
        organization: org.to_owned(),
        project: project.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gh(owner: &str) -> Option<Remote> {
        Some(Remote::GitHub {
            owner: owner.into(),
        })
    }

    fn az(org: &str, project: &str) -> Option<Remote> {
        Some(Remote::AzureDevOps {
            organization: org.into(),
            project: project.into(),
        })
    }

    #[test]
    fn github_https_ssh_and_scheme_forms() {
        for url in [
            "https://github.com/GravionLabs/poimen.git",
            "https://github.com/GravionLabs/poimen",
            "https://user@github.com/GravionLabs/poimen/",
            "git@github.com:GravionLabs/poimen.git",
            "ssh://git@github.com/GravionLabs/poimen.git",
            "ssh://git@github.com:22/GravionLabs/poimen.git",
            "  https://GitHub.com/GravionLabs/poimen\n",
        ] {
            assert_eq!(parse(url), gh("GravionLabs"), "{url}");
        }
    }

    #[test]
    fn azure_devops_https_with_and_without_user() {
        assert_eq!(
            parse("https://dev.azure.com/acme/Shop/_git/web"),
            az("acme", "Shop")
        );
        assert_eq!(
            parse("https://acme@dev.azure.com/acme/Shop/_git/web"),
            az("acme", "Shop")
        );
    }

    #[test]
    fn azure_devops_project_names_are_url_decoded() {
        assert_eq!(
            parse("https://dev.azure.com/acme/FLEX%20Business/_git/web"),
            az("acme", "FLEX Business")
        );
    }

    #[test]
    fn azure_devops_legacy_visualstudio_host() {
        assert_eq!(
            parse("https://acme.visualstudio.com/Shop/_git/web"),
            az("acme", "Shop")
        );
        assert_eq!(
            parse("https://acme.visualstudio.com/DefaultCollection/Shop/_git/web"),
            az("acme", "Shop")
        );
    }

    #[test]
    fn azure_devops_ssh() {
        assert_eq!(
            parse("git@ssh.dev.azure.com:v3/acme/Shop/web"),
            az("acme", "Shop")
        );
        assert_eq!(
            parse("git@ssh.dev.azure.com:v3/acme/FLEX%20Business/web"),
            az("acme", "FLEX Business")
        );
    }

    #[test]
    fn unknown_hosts_and_incomplete_urls_are_none() {
        for url in [
            "",
            "/home/jk/repo",
            "https://gitlab.com/a/b.git",
            "git@gitlab.com:a/b.git",
            "https://github.com/OnlyOwner",
            "https://dev.azure.com/acme",
            "https://dev.azure.com/acme/Shop",
            "git@ssh.dev.azure.com:v3/acme",
        ] {
            assert_eq!(parse(url), None, "{url}");
        }
    }

    #[test]
    fn same_ignores_case_and_provider_must_match() {
        assert!(gh("GravionLabs").unwrap().same(&gh("gravionlabs").unwrap()));
        assert!(
            az("Acme", "Shop")
                .unwrap()
                .same(&az("acme", "SHOP").unwrap())
        );
        assert!(!gh("a").unwrap().same(&gh("b").unwrap()));
        assert!(!az("acme", "x").unwrap().same(&az("acme", "y").unwrap()));
        assert!(!gh("acme").unwrap().same(&az("acme", "acme").unwrap()));
    }
}
