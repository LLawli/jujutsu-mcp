//! Network tools: fetch and push through the git remote.
//!
//! Both can run long: a push signs every outgoing commit, and with a
//! hardware key each signature waits for a touch. While jj runs they send
//! progress notifications when the client asked for them, and a client
//! cancellation kills the jj process.

use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ProgressNotificationParam};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::time::{Instant, MissedTickBehavior, interval_at};

use crate::jj::{JjOutput, WriteGuard};
use crate::repo::RepoPath;
use crate::server::JjServer;
use crate::tools::{ToolError, non_empty, text_result};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GitFetchParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Remote to fetch from. Defaults to jj's configured fetch remote.
    #[serde(default)]
    pub remote: Option<String>,
    /// Fetch from every remote. Cannot be combined with `remote`.
    #[serde(default)]
    pub all_remotes: bool,
    /// Only these branches (jj string patterns, glob by default).
    #[serde(default)]
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GitPushParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Bookmarks to push; a bookmark the remote does not have yet is
    /// created there.
    #[serde(default)]
    pub bookmarks: Vec<String>,
    /// Push these revisions under generated bookmark names.
    #[serde(default)]
    pub changes: Vec<String>,
    /// Remote to push to. Defaults to jj's configured push remote.
    #[serde(default)]
    pub remote: Option<String>,
    /// Report what would be pushed and how many commits would be signed,
    /// without pushing. Use it to warn the user before a push that asks for
    /// one security-key touch per commit.
    #[serde(default)]
    pub dry_run: bool,
}

/// Summary line that opens every `git_push` result.
pub fn commits_to_sign_line(count: usize) -> String {
    format!("commits to sign: {count}")
}

pub fn git_fetch_args(params: &GitFetchParams) -> Result<Vec<String>, ToolError> {
    if params.remote.is_some() && params.all_remotes {
        return Err(ToolError::InvalidParams(
            "remote cannot be combined with all_remotes".to_owned(),
        ));
    }
    let mut args = vec!["git".to_owned(), "fetch".to_owned()];
    if let Some(remote) = &params.remote {
        args.push(format!("--remote={}", non_empty("remote", remote)?));
    }
    if params.all_remotes {
        args.push("--all-remotes".to_owned());
    }
    for branch in &params.branches {
        args.push(format!("--branch={}", non_empty("branches", branch)?));
    }
    Ok(args)
}

/// argv for the push itself, or for its dry run when `dry_run` is set.
pub fn git_push_args(params: &GitPushParams, dry_run: bool) -> Result<Vec<String>, ToolError> {
    let mut args = vec!["git".to_owned(), "push".to_owned()];
    if let Some(remote) = &params.remote {
        args.push(format!("--remote={}", non_empty("remote", remote)?));
    }
    for bookmark in &params.bookmarks {
        args.push(format!("--bookmark={}", non_empty("bookmarks", bookmark)?));
    }
    for change in &params.changes {
        args.push(format!("--change={}", non_empty("changes", change)?));
    }
    if dry_run {
        args.push("--dry-run".to_owned());
    }
    Ok(args)
}

/// Commit ids (as printed, abbreviated) that `jj git push --dry-run` would
/// make remote bookmarks point at. Deletions have no target.
pub fn push_targets(dry_run_output: &str) -> Vec<String> {
    dry_run_output
        .lines()
        .filter_map(|line| {
            let change = line.trim().strip_prefix("bookmark: ")?;
            let detail = change.strip_suffix(']')?;
            let detail = &detail[detail.rfind('[')? + 1..];
            if let Some(target) = detail.strip_prefix("add to ") {
                Some(target.trim().to_owned())
            } else if detail.starts_with("move ") {
                // "move forward|sideways|backward from A to H"
                detail.rsplit_once(" to ").map(|(_, h)| h.trim().to_owned())
            } else {
                // "delete from A": the remote bookmark goes away, nothing is signed.
                None
            }
        })
        .filter(|target| !target.is_empty())
        .collect()
}

/// Revset of the commits a push would send for the first time: reachable
/// from `targets` but from no bookmark of a real remote. The `git` remote
/// is excluded because in a colocated repository jj mirrors every local
/// bookmark there, which would make every commit look already pushed.
fn unpushed_revset(targets: &[String]) -> String {
    let ids = targets
        .iter()
        .map(|id| {
            let escaped = id.replace('\\', "\\\\").replace('"', "\\\"");
            format!("commit_id(\"{escaped}\")")
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "(::({ids}) ~ ::(remote_bookmarks() ~ remote_bookmarks(remote=exact:\"git\"))) ~ root()"
    )
}

/// jj exits 0 when a requested bookmark does not exist: it warns and pushes
/// the rest, or nothing. An agent that mistyped a name must hear about it
/// instead of reading a success, so the push stops before pushing anything.
fn reject_unmatched_bookmarks(dry_run: &JjOutput) -> Result<(), ToolError> {
    const WARNING: &str = "Warning: No matching bookmarks for names:";
    match dry_run
        .stderr
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(WARNING))
    {
        Some(line) => Err(ToolError::InvalidParams(
            line.trim_start_matches("Warning: ").to_owned(),
        )),
        None => Ok(()),
    }
}

/// What a long call reports while it runs.
struct Progress<'a> {
    context: &'a RequestContext<RoleServer>,
    interval: Duration,
    /// What is being done, e.g. `git push (3 commits to sign)`.
    label: String,
}

impl JjServer {
    /// Waits for the repository's write queue, giving up if the client
    /// cancels first so a cancelled call never runs anything.
    async fn lock_write(
        &self,
        repo: &RepoPath,
        context: &RequestContext<RoleServer>,
    ) -> Result<WriteGuard, ToolError> {
        tokio::select! {
            biased;
            () = context.ct.cancelled() => Err(ToolError::Cancelled),
            guard = self.write_queue.lock(repo) => Ok(guard),
        }
    }

    /// Runs jj, sending a progress notification every interval when the
    /// client sent a progress token. A client cancellation drops the runner
    /// future, which kills the jj process, and returns `Cancelled`. There is
    /// no timeout: a push may wait for security-key touches.
    async fn run_watched(
        &self,
        repo: &RepoPath,
        args: &[String],
        context: &RequestContext<RoleServer>,
        progress: Option<Progress<'_>>,
    ) -> Result<JjOutput, ToolError> {
        let work = self.runner.run(repo, args);
        tokio::pin!(work);

        let token = progress
            .as_ref()
            .and_then(|progress| progress.context.meta.get_progress_token());
        let period = progress
            .as_ref()
            .map_or(Duration::from_secs(3600), |progress| {
                progress.interval.max(Duration::from_millis(1))
            });
        let started = Instant::now();
        let mut ticker = interval_at(started + period, period);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut step = 0.0_f64;

        loop {
            tokio::select! {
                biased;
                () = context.ct.cancelled() => return Err(ToolError::Cancelled),
                result = &mut work => return Ok(result?),
                _ = ticker.tick(), if token.is_some() => {
                    let (Some(token), Some(progress)) = (&token, &progress) else { continue };
                    step += 1.0;
                    let message = format!(
                        "{} running, {}s elapsed",
                        progress.label,
                        started.elapsed().as_secs()
                    );
                    let notice = ProgressNotificationParam::new(token.clone(), step)
                        .with_message(message);
                    // A client that stopped listening must not fail the call.
                    let _ = progress.context.peer.notify_progress(notice).await;
                }
            }
        }
    }

    /// Number of commits the dry run would make jj sign: the commits
    /// reachable from the push targets that no real remote has yet.
    async fn commits_to_sign(
        &self,
        repo: &RepoPath,
        dry_run: &JjOutput,
        context: &RequestContext<RoleServer>,
    ) -> Result<usize, ToolError> {
        let targets = push_targets(&format!(
            "{}
{}",
            dry_run.stdout, dry_run.stderr
        ));
        if targets.is_empty() {
            return Ok(0);
        }
        let args = vec![
            "log".to_owned(),
            "--no-graph".to_owned(),
            "--template".to_owned(),
            "commit_id ++ \"\\n\"".to_owned(),
            format!("--revisions={}", unpushed_revset(&targets)),
        ];
        let output = self.run_watched(repo, &args, context, None).await?;
        Ok(output
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count())
    }
}

/// Result of `git_push`: the signing summary, then jj's own output.
fn push_result(count: usize, output: JjOutput) -> CallToolResult {
    let mut result = text_result(output);
    result
        .content
        .insert(0, ContentBlock::text(commits_to_sign_line(count)));
    result
}

#[tool_router(router = remote_router, vis = "pub(crate)")]
impl JjServer {
    /// Fetch from git remotes.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn git_fetch(
        &self,
        Parameters(params): Parameters<GitFetchParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = git_fetch_args(&params)?;
        let progress = Progress {
            context: &context,
            interval: self.progress_interval,
            label: "git fetch".to_owned(),
        };
        let _guard = self.lock_write(&repo, &context).await?;
        let output = self
            .run_watched(&repo, &args, &context, Some(progress))
            .await?;
        Ok(text_result(output))
    }

    /// Push bookmarks to a git remote. Every outgoing commit is signed and
    /// each signature may wait for a security-key touch; the result opens
    /// with `commits to sign: N`. Run with `dry_run` first to tell the user
    /// how many touches to expect.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn git_push(
        &self,
        Parameters(params): Parameters<GitPushParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let push_args = git_push_args(&params, false)?;
        let dry_args = git_push_args(&params, true)?;

        // Held through dry run, count and push so the count matches what is pushed.
        let _guard = self.lock_write(&repo, &context).await?;
        let dry_run = self.run_watched(&repo, &dry_args, &context, None).await?;
        reject_unmatched_bookmarks(&dry_run)?;
        let count = self.commits_to_sign(&repo, &dry_run, &context).await?;
        if params.dry_run {
            return Ok(push_result(count, dry_run));
        }

        let plural = if count == 1 { "" } else { "s" };
        let progress = Progress {
            context: &context,
            interval: self.progress_interval,
            label: format!("git push ({count} commit{plural} to sign, touch the key if asked)"),
        };
        let output = self
            .run_watched(&repo, &push_args, &context, Some(progress))
            .await?;
        Ok(push_result(count, output))
    }
}
