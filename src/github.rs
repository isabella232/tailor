// Copyright 2017 CoreOS, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use chrono::prelude::*;
use config;
use errors::*;
use expr;
use expr::ast::Value;
use github_rs::client::Github;
use serde_json;
use worker;

#[derive(Clone, Value)]
struct PullRequest {
    user: User,
    title: String,
    body: String,
    commits: Vec<Commit>,
    comments: Vec<Comment>,
}

#[derive(Deserialize)]
struct RawPullRequest {
    user: User,
    title: String,
    body: String,
}

#[derive(Clone, Deserialize, Value)]
struct User {
    login: String,
}

#[derive(Clone, Deserialize, Value)]
struct Commit {
    sha: String,
    commit: CommitBody,
    author: User,
    committer: User,
}

#[derive(Clone, Deserialize, Value)]
struct CommitBody {
    author: Author,
    committer: Author,
    message: String,
}

#[derive(Clone, Deserialize, Value)]
struct Author {
    name: String,
    email: String,
    date: DateTime<Utc>,
}

#[derive(Clone, Deserialize, Value)]
struct Comment {
    user: User,
    body: String,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct Collaborator {
    permission: Permission,
}

#[derive(Deserialize, PartialEq)]
enum Permission {
    #[serde(rename = "admin")]
    Admin,
    #[serde(rename = "write")]
    Write,
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "none")]
    None,
}

pub fn validate_pull_request(
    job: &worker::PullRequestJob,
    client: &Github,
    repo: &config::Repo,
) -> Result<Vec<String>> {
    let pr = fetch_pull_request(&client, &repo, job.number)?;
    let exemptions = find_exemptions(&client, &repo, &pr)?;

    let mut failures = Vec::new();
    let input = pr.clone().into();
    for rule in repo.rules.iter().filter(
        |rule| !exemptions.contains(&rule.name),
    )
    {
        if !expr::eval(&rule.expression, &input).chain_err(|| {
            format!(
                r#"Failed to run "{}" from "{}/{}""#,
                rule.name,
                repo.owner,
                repo.repo
            )
        })?
        {
            failures.push(format!("Failed {} ({})", rule.name, rule.description))
        }
    }
    Ok(failures)
}

fn find_exemptions(client: &Github, repo: &config::Repo, pr: &PullRequest) -> Result<Vec<String>> {
    let mut exemptions = Vec::new();
    for comment in &pr.comments {
        // TODO ALEX
        if (&(comment.body)).starts_with("tailor disable") {
            let mut split = comment.body.as_str().split("tailor disable");
            split.next();
            if let Some(disabled_check) = split.next() {
                let collaborator: Collaborator = match client
                    .get()
                    .repos()
                    .owner(&repo.owner)
                    .repo(&repo.repo)
                    .collaborators()
                    .username(&comment.user.login)
                    .permission()
                    .execute() {
                    Ok((_, _, Some(json))) => serde_json::from_value(json)?,
                    Ok((_, status, _)) => {
                        bail!(format!("Could not get collaborator data: HTTP {}", status))
                    }
                    Err(err) => bail!(err),
                };
                if collaborator.permission == Permission::Admin {
                    exemptions.push(disabled_check.trim().to_string());
                }
            }
        }
    }

    Ok(exemptions)
}

fn fetch_pull_request(client: &Github, repo: &config::Repo, number: usize) -> Result<PullRequest> {
    let pr: RawPullRequest = match client
        .get()
        .repos()
        .owner(&repo.owner)
        .repo(&repo.repo)
        .pulls()
        .number(&number.to_string())
        .execute() {
        Ok((_, _, Some(json))) => json,
        Ok((_, status, _)) => bail!(format!("Could not get pull request: HTTP {}", status)),
        Err(err) => bail!(err),
    };

    let commits: Vec<Commit> = match client
        .get()
        .repos()
        .owner(&repo.owner)
        .repo(&repo.repo)
        .pulls()
        .number(&number.to_string())
        .commits()
        .execute() {
        Ok((_, _, Some(json))) => json,
        Ok((_, status, _)) => {
            bail!(format!(
                "Could not get pull request commits: HTTP {}",
                status
            ))
        }
        Err(err) => bail!(err),
    };

    let comments: Vec<Comment> = match client
        .get()
        .repos()
        .owner(&repo.owner)
        .repo(&repo.repo)
        .issues()
        .number(&number.to_string())
        .comments()
        .execute() {
        Ok((_, _, Some(json))) => json,
        Ok((_, status, _)) => {
            bail!(format!(
                "Could not get pull request comments: HTTP {}",
                status
            ))
        }
        Err(err) => bail!(err),
    };

    Ok(PullRequest {
        user: pr.user,
        title: pr.title,
        body: pr.body,
        commits,
        comments,
    })
}

/*
#[derive(Debug, Deserialize)]
struct GithubErrorResponse {
    message: String,
    errors: Vec<GithubError>,
}

#[derive(Debug, Deserialize)]
struct GithubError{
    resource: String,
    code: String,
    field: String,
    message: String,
}

Ok((_, StatusCode::Created, _)) => Ok(()),
Ok((_, _, Some(res))) => match serde_json::from_value::<GithubErrorResponse>(res) {
    Ok(error) => Err(format!("Received error from API: {:?}", error)),
    Err(err) => Err(format!("Failed to parse error response: {}", err)),
},
Ok((_, _, None)) => Err("Empty error response received".to_string()),
Err(err) => Err(format!("Failed to send request: {}", err)),
*/
