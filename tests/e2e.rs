use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::time::{Duration, SystemTime};

use filetime::FileTime;
use tempfile::TempDir;

const OLD_DAYS: u64 = 30;
const SECONDS_PER_DAY: u64 = 86_400;
const CARGO_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n";

struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self::with_config("")
    }

    fn with_config(extra: &str) -> Self {
        let sandbox = Self {
            dir: TempDir::new().unwrap(),
        };
        fs::create_dir_all(sandbox.code()).unwrap();
        fs::write(
            sandbox.config(),
            format!(
                "roots = [\"{}\"]\ndocker = false\ntreehouse = false\nnotify = false\n{extra}",
                sandbox.code().display()
            ),
        )
        .unwrap();
        sandbox
    }

    fn code(&self) -> PathBuf {
        self.dir.path().join("code")
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    fn state_json(&self, name: &str) -> serde_json::Value {
        let path = self.dir.path().join("state/reclaim").join(name);
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap()
    }

    fn reclaim(&self, args: &[&str]) -> Output {
        let out = self.reclaim_unchecked(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn reclaim_unchecked(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_reclaim"))
            .arg("--config")
            .arg(self.config())
            .args(args)
            .env("XDG_STATE_HOME", self.dir.path().join("state"))
            .output()
            .unwrap()
    }
}

fn count_of(totals: &serde_json::Value, kind: &str) -> u64 {
    totals
        .as_array()
        .unwrap()
        .iter()
        .find(|total| total["kind"] == kind)
        .map_or(0, |total| total["count"].as_u64().unwrap())
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Sets every modification time in the tree to `OLD_DAYS` ago.
fn age(path: &Path) {
    let old = SystemTime::now() - Duration::from_secs(OLD_DAYS * SECONDS_PER_DAY);
    let time = FileTime::from_system_time(old);
    for entry in walk(path) {
        filetime::set_symlink_file_times(&entry, time, time).unwrap();
    }
}

fn walk(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    if path.is_dir() && !path.is_symlink() {
        for entry in fs::read_dir(path).unwrap() {
            paths.extend(walk(&entry.unwrap().path()));
        }
    }
    paths
}

/// A repository with a pushed `main`, plus one worktree per case.
fn repo_with_worktrees(sandbox: &Sandbox) -> PathBuf {
    let remote = sandbox.dir.path().join("remote.git");
    git(
        sandbox.dir.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    let repo = sandbox.code().join("app");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    write(&repo.join(".gitignore"), "build/\n.env\ntools/target/\n");
    write(&repo.join("README"), "app\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "-u", "origin", "main"]);

    for name in [
        "landed", "secrets", "dirty", "unpushed", "busy", "locked", "recent",
    ] {
        let path = repo.join(".worktrees").join(name);
        git(
            &repo,
            &["worktree", "add", "-b", name, path.to_str().unwrap()],
        );
    }
    let worktree = |name: &str| repo.join(".worktrees").join(name);

    write(&worktree("landed").join("feature"), "done\n");
    git(&worktree("landed"), &["add", "feature"]);
    git(&worktree("landed"), &["commit", "-m", "feature"]);
    git(&worktree("landed"), &["push", "origin", "landed"]);
    write(
        &worktree("landed").join("build/output.bin"),
        "ignored build output\n",
    );
    write(
        &worktree("landed").join("tools/target/CACHEDIR.TAG"),
        CARGO_TAG,
    );

    write(&worktree("secrets").join(".env"), "TOKEN=only-copy\n");

    write(&worktree("dirty").join("notes.txt"), "not committed\n");

    write(&worktree("unpushed").join("work"), "local only\n");
    git(&worktree("unpushed"), &["add", "work"]);
    git(&worktree("unpushed"), &["commit", "-m", "local"]);

    git(
        &repo,
        &["worktree", "lock", worktree("locked").to_str().unwrap()],
    );

    age(&repo.join(".worktrees"));
    age(&repo.join(".git/worktrees"));
    filetime::set_file_mtime(worktree("recent"), FileTime::now()).unwrap();
    repo
}

/// Build outputs that have not been touched for `OLD_DAYS`, and one that was just used.
fn build_outputs(sandbox: &Sandbox) -> Vec<(PathBuf, bool)> {
    let code = sandbox.code();
    write(&code.join("rusty/Cargo.toml"), "[package]\n");
    write(&code.join("rusty/target/CACHEDIR.TAG"), CARGO_TAG);
    write(&code.join("rusty/target/debug/deps/libx.rlib"), "x");
    write(&code.join("web/package.json"), "{}");
    write(&code.join("web/node_modules/left-pad/index.js"), "x");
    write(&code.join("svc/svc.csproj"), "<Project/>");
    write(&code.join("svc/bin/Debug/svc.dll"), "x");
    write(&code.join("svc/obj/project.assets.json"), "{}");
    write(&code.join("tools/bin/script.sh"), "echo not a build output");
    write(&code.join("fresh/Cargo.toml"), "[package]\n");
    write(&code.join("fresh/target/CACHEDIR.TAG"), CARGO_TAG);
    write(&code.join("served/package.json"), "{}");
    write(&code.join("served/node_modules/express/package.json"), "{}");
    write(&code.join("edited/package.json"), "{}");
    write(&code.join("edited/node_modules/react/package.json"), "{}");
    write(&code.join("edited/src/app.ts"), "export {}");
    let rooted = code.join("rooted");
    write(&rooted.join(".gitignore"), "target/\nnode_modules/\n");
    write(&rooted.join("Cargo.toml"), "[package]\n");
    write(&rooted.join("fixture/package.json"), "{}");
    write(
        &rooted.join("fixture/node_modules/dep/index.js"),
        "committed fixture",
    );
    write(&rooted.join("tool/tool.csproj"), "<Project/>");
    git(&rooted, &["init", "-b", "main"]);
    git(&rooted, &["add", "."]);
    git(&rooted, &["add", "--force", "fixture/node_modules"]);
    git(&rooted, &["commit", "-m", "init"]);
    write(&rooted.join("target/CACHEDIR.TAG"), CARGO_TAG);
    write(&rooted.join("target/debug/rooted"), "x");
    write(
        &rooted.join("tool/bin/notes.txt"),
        "neither committed nor ignored",
    );
    for project in [
        "rusty", "web", "svc", "tools", "fresh", "served", "edited", "rooted",
    ] {
        age(&code.join(project));
    }
    write(
        &code.join("fresh/target/debug/deps/libnew.rlib"),
        "just built",
    );
    filetime::set_file_atime(
        code.join("served/node_modules/express/package.json"),
        FileTime::now(),
    )
    .unwrap();
    filetime::set_file_mtime(code.join("edited/src/app.ts"), FileTime::now()).unwrap();

    vec![
        (code.join("served/node_modules"), false),
        (code.join("edited/node_modules"), false),
        (code.join("rusty/target"), true),
        (code.join("web/node_modules"), true),
        (code.join("svc/bin"), true),
        (code.join("svc/obj"), true),
        (code.join("tools/bin"), false),
        (code.join("fresh/target"), false),
        (rooted.join("target"), true),
        (rooted.join("fixture/node_modules"), false),
        (rooted.join("tool/bin"), false),
    ]
}

struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn run_deletes_only_what_is_safe_and_status_deletes_nothing() {
    let sandbox = Sandbox::new();
    let repo = repo_with_worktrees(&sandbox);
    let outputs = build_outputs(&sandbox);
    let busy = repo.join(".worktrees/busy");
    let _sleeper = Sleeper(
        Command::new("sleep")
            .arg("120")
            .current_dir(&busy)
            .spawn()
            .unwrap(),
    );

    let status = sandbox.reclaim(&["status", "--all"]);
    let status = String::from_utf8_lossy(&status.stdout);
    for (path, _) in &outputs {
        assert!(path.exists(), "status deleted {}", path.display());
    }
    assert!(
        repo.join(".worktrees/landed").exists(),
        "status deleted a worktree"
    );
    assert!(status.contains("every commit is on a remote"), "{status}");
    assert!(status.contains("in use by sleep"), "{status}");
    assert!(status.contains("ignored files: .env"), "{status}");
    assert!(
        status.contains("git tracks or does not ignore dep/index.js"),
        "{status}"
    );
    assert!(
        status.contains("git tracks or does not ignore notes.txt"),
        "{status}"
    );

    sandbox.reclaim(&["scan"]);
    let plan = sandbox.state_json("plan.json");
    assert_eq!(count_of(&plan["reclaimable"], "worktree"), 1, "{plan}");
    assert_eq!(count_of(&plan["reclaimable"], "dotnet"), 2, "{plan}");
    assert!(plan["reclaimable_bytes"].as_u64().unwrap() > 0, "{plan}");
    for (path, _) in &outputs {
        assert!(path.exists(), "scan deleted {}", path.display());
    }

    sandbox.reclaim(&["run"]);

    let last_run = sandbox.state_json("last-run.json");
    assert!(last_run["freed_bytes"].as_u64().unwrap() > 0, "{last_run}");
    assert_eq!(count_of(&last_run["deleted"], "worktree"), 1, "{last_run}");
    assert_eq!(count_of(&last_run["deleted"], "node"), 1, "{last_run}");
    assert_eq!(last_run["failed"], 0, "{last_run}");
    let plan = sandbox.state_json("plan.json");
    assert_eq!(
        plan["reclaimable_bytes"], 0,
        "nothing left to delete after a run: {plan}"
    );

    for (path, deleted) in &outputs {
        assert_eq!(!path.exists(), *deleted, "{}", path.display());
    }
    let worktree = |name: &str| repo.join(".worktrees").join(name);
    assert!(
        !worktree("landed").exists(),
        "landed worktree, with ignored build output, is removed"
    );
    for kept in ["secrets", "dirty", "unpushed", "busy", "locked", "recent"] {
        assert!(worktree(kept).exists(), "{kept} worktree must be kept");
    }
    assert!(worktree("dirty").join("notes.txt").exists());
    assert!(worktree("secrets").join(".env").exists());

    let listing = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&listing.stdout);
    assert!(
        !listing.contains("/landed"),
        "git no longer lists the removed worktree: {listing}"
    );
}

/// An old `node_modules` that a run deletes.
fn stale_node_modules(sandbox: &Sandbox, project: &str) -> PathBuf {
    let dir = sandbox.code().join(project);
    write(&dir.join("package.json"), "{}");
    write(&dir.join("node_modules/left-pad/index.js"), "x");
    age(&dir);
    dir.join("node_modules")
}

#[test]
fn low_disk_check_does_not_rescan_right_after_a_run() {
    let sandbox = Sandbox::with_config("low_disk_free_percent = 100\n");
    let first = stale_node_modules(&sandbox, "first");

    sandbox.reclaim(&["run", "--only-if-low-disk"]);
    assert!(!first.exists(), "the first low-disk run cleans up");

    let second = stale_node_modules(&sandbox, "second");
    let out = sandbox.reclaim(&["run", "--only-if-low-disk"]);
    let out = String::from_utf8_lossy(&out.stdout);
    assert!(
        second.exists(),
        "a low-disk run right after another must not scan again"
    );
    assert!(out.contains("not scanning again"), "{out}");

    sandbox.reclaim(&["run"]);
    assert!(!second.exists(), "the daily run is not held back");
}

const ALLOW_FILE_SUBMODULES: [&str; 2] = ["-c", "protocol.file.allow=always"];

/// A pushed repository `lib`, with `.env` ignored, used as a submodule.
fn submodule_remote(sandbox: &Sandbox) -> PathBuf {
    let remote = sandbox.dir.path().join("lib.git");
    git(
        sandbox.dir.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    let lib = sandbox.dir.path().join("lib");
    fs::create_dir_all(&lib).unwrap();
    git(&lib, &["init", "-b", "main"]);
    write(&lib.join(".gitignore"), ".env\n");
    write(&lib.join("lib.rs"), "pub fn f() {}\n");
    git(&lib, &["add", "."]);
    git(&lib, &["commit", "-m", "lib"]);
    git(&lib, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&lib, &["push", "-u", "origin", "main"]);
    remote
}

#[test]
fn worktrees_with_submodules_are_checked_inside_and_removed() {
    let sandbox = Sandbox::new();
    let lib_remote = submodule_remote(&sandbox);
    let remote = sandbox.dir.path().join("app.git");
    git(
        sandbox.dir.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    let repo = sandbox.code().join("app");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    let mut add = ALLOW_FILE_SUBMODULES.to_vec();
    add.extend([
        "submodule",
        "add",
        lib_remote.to_str().unwrap(),
        "vendor/lib",
    ]);
    git(&repo, &add);
    git(&repo, &["commit", "-m", "add lib"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "-u", "origin", "main"]);

    let worktree = |name: &str| repo.join(".worktrees").join(name);
    for name in ["clean", "secret", "branched"] {
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                name,
                worktree(name).to_str().unwrap(),
            ],
        );
        let mut init = ALLOW_FILE_SUBMODULES.to_vec();
        init.extend(["submodule", "update", "--init"]);
        git(&worktree(name), &init);
    }
    write(
        &worktree("secret").join("vendor/lib/.env"),
        "TOKEN=only-copy\n",
    );
    let lib = worktree("branched").join("vendor/lib");
    git(&lib, &["checkout", "-b", "local-work"]);
    write(&lib.join("wip.rs"), "only on a local branch\n");
    git(&lib, &["add", "wip.rs"]);
    git(&lib, &["commit", "-m", "wip"]);
    git(&lib, &["checkout", "--detach", "HEAD~1"]);
    age(&repo.join(".worktrees"));
    age(&repo.join(".git/worktrees"));

    let status = sandbox.reclaim(&["status"]);
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status.contains("vendor/lib: ignored files: .env"),
        "{status}"
    );
    assert!(
        status.contains("vendor/lib: 1 commit on no remote"),
        "{status}"
    );

    sandbox.reclaim(&["run"]);
    assert!(
        !worktree("clean").exists(),
        "a clean worktree with a pushed submodule is removed"
    );
    assert!(
        worktree("secret").join("vendor/lib/.env").exists(),
        "work inside a submodule is kept"
    );
    assert!(
        worktree("branched").exists(),
        "a submodule commit on a local branch is kept with its worktree"
    );
    assert_eq!(sandbox.state_json("last-run.json")["failed"], 0);
}

fn decision_reason(sandbox: &Sandbox, worktree: &Path) -> String {
    let plan = sandbox.state_json("plan.json");
    let target = worktree.display().to_string();
    plan["needs_decision"]
        .as_array()
        .unwrap()
        .iter()
        .find(|decision| decision["path"] == target.as_str())
        .unwrap_or_else(|| panic!("no decision for {target}: {plan}"))["reason"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn remove_deletes_an_accepted_decision_and_refuses_a_changed_one() {
    let sandbox = Sandbox::new();
    let remote = sandbox.dir.path().join("app.git");
    git(
        sandbox.dir.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    let repo = sandbox.code().join("app");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    write(&repo.join(".gitignore"), ".env\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "-u", "origin", "main"]);

    let worktree = |name: &str| repo.join(".worktrees").join(name);
    for name in ["accepted", "changed"] {
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                name,
                worktree(name).to_str().unwrap(),
            ],
        );
        write(&worktree(name).join(".env"), "TOKEN=old\n");
    }
    write(&worktree("accepted").join("local"), "only here\n");
    git(&worktree("accepted"), &["add", "local"]);
    git(&worktree("accepted"), &["commit", "-m", "local"]);
    age(&repo.join(".worktrees"));
    age(&repo.join(".git/worktrees"));
    sandbox.reclaim(&["scan"]);

    let accepted = decision_reason(&sandbox, &worktree("accepted"));
    assert!(accepted.contains("ignored files: .env"), "{accepted}");
    assert!(
        accepted.contains("1 commit on no remote"),
        "every blocker is listed: {accepted}"
    );
    sandbox.reclaim(&[
        "remove",
        worktree("accepted").to_str().unwrap(),
        "--accept",
        &accepted,
    ]);
    assert!(!worktree("accepted").exists());
    let plan = sandbox.state_json("plan.json");
    assert!(
        !plan.to_string().contains("/accepted"),
        "the decision is gone: {plan}"
    );

    let seen = decision_reason(&sandbox, &worktree("changed"));
    write(
        &worktree("changed").join("notes.txt"),
        "written after the scan\n",
    );
    let out = sandbox.reclaim_unchecked(&[
        "remove",
        worktree("changed").to_str().unwrap(),
        "--accept",
        &seen,
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("changed since the scan"));
    assert!(worktree("changed").join("notes.txt").exists());
}

/// A worktree directory that a run cannot see (moved, in a private /tmp, on an unmounted disk)
/// keeps its git metadata: the index, HEAD and submodule git directories live there.
#[test]
fn a_worktree_git_cannot_find_is_not_forgotten() {
    let sandbox = Sandbox::new();
    let repo = sandbox.code().join("app");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    write(&repo.join("README"), "app\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);
    let worktree = repo.join(".worktrees/away");
    git(
        &repo,
        &["worktree", "add", "-b", "away", worktree.to_str().unwrap()],
    );
    write(&worktree.join("staged"), "only in the index\n");
    git(&worktree, &["add", "staged"]);
    let elsewhere = sandbox.dir.path().join("away");
    fs::rename(&worktree, &elsewhere).unwrap();

    sandbox.reclaim(&["run"]);

    fs::rename(&elsewhere, &worktree).unwrap();
    let status = Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&status.stdout), "A  staged\n");
}
