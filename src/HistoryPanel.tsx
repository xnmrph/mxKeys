import { useCallback, useEffect, useRef, useState } from "react";
import { confirmAction } from "./dialogs";
import { invoke } from "@tauri-apps/api/core";
import type { ChangedFile, FileDiff, GitOp, GitOpResult, History } from "./types";

const KIND_LABEL: Record<string, string> = {
  added: "new", modified: "changed", deleted: "deleted", renamed: "renamed", conflicted: "conflict", type_change: "changed",
};

function ago(seconds: number): string {
  const d = Date.now() / 1000 - seconds;
  if (d < 90) return "just now";
  if (d < 3600) return `${Math.round(d / 60)} minutes ago`;
  if (d < 86400 * 2) return `${Math.round(d / 3600)} hours ago`;
  return `${Math.round(d / 86400)} days ago`;
}

function plural(n: number, one: string, many: string) {
  return `${n} ${n === 1 ? one : many}`;
}

/** Where the upload goes, in plain words (“GitHub (xnmrph/corne)”). */
function serverName(url: string | null): string {
  if (!url) return "the server";
  // A folder on this computer or a network share.
  if (/^(\/|[A-Za-z]:[\\/]|file:)/.test(url)) return `the folder “${url.replace(/[\\/]+$/, "").split(/[\\/]/).pop()}”`;
  const m = url.match(/^(?:https?:\/\/|ssh:\/\/)?(?:[^@/]+@)?([^/:]+)[/:](.+?)(?:\.git)?\/?$/);
  if (!m) return url;
  const host = m[1].replace(/^www\./, "");
  const name = host === "github.com" ? "GitHub" : host === "gitlab.com" ? "GitLab" : host === "codeberg.org" ? "Codeberg" : host;
  return `${name} (${m[2]})`;
}

export function HistoryPanel({ repoRoot, changes, onFilesChanged }: {
  repoRoot: string;
  /** Bumped on every edit in the app. */
  changes: number;
  /** Files changed on disk because of a git operation (pull, undo changes). */
  onFilesChanged: () => void;
}) {
  const [h, setH] = useState<History | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [fetchProblem, setFetchProblem] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [messageEdited, setMessageEdited] = useState(false);
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<string | null>(null);
  const [diffs, setDiffs] = useState<Record<string, FileDiff[]>>({});
  const [identity, setIdentity] = useState({ name: "", email: "" });
  const loading = useRef(false);
  const filesKey = useRef("");

  const refresh = useCallback(async () => {
    if (loading.current) return;
    loading.current = true;
    try {
      const next = await invoke<History>("git_history", { repoPath: repoRoot });
      setH(next);
      // Line views stay open unless the changes themselves changed.
      const key = JSON.stringify(next.files);
      if (key !== filesKey.current) {
        filesKey.current = key;
        setDiffs({});
      }
    } catch (e) {
      setError(String(e));
    } finally {
      loading.current = false;
    }
  }, [repoRoot]);

  // Refresh after edits, and now and then for changes made outside the app.
  useEffect(() => { void refresh(); }, [refresh, changes]);
  useEffect(() => {
    const t = setInterval(() => { if (!busy) void refresh(); }, 5000);
    return () => clearInterval(t);
  }, [refresh, busy]);
  // Check the server in the background (silently: offline is fine): now, every few
  // minutes, and when the window comes back into focus.
  useEffect(() => {
    let last = 0;
    const check = () => {
      if (Date.now() - last < 60_000) return;
      last = Date.now();
      invoke<GitOpResult>("git_run", { repoPath: repoRoot, op: { op: "fetch" } })
        .then(() => { setFetchProblem(null); void refresh(); })
        .catch((e) => setFetchProblem(String(e)));
    };
    check();
    const t = setInterval(check, 180_000);
    window.addEventListener("focus", check);
    return () => { clearInterval(t); window.removeEventListener("focus", check); };
  }, [repoRoot, refresh]);
  // Re-load an open line view after it was cleared by a change.
  useEffect(() => {
    if (open && !diffs[open]) {
      invoke<FileDiff[]>("git_diff", { repoPath: repoRoot, file: open })
        .then((d) => setDiffs((x) => ({ ...x, [open]: d })))
        .catch((e) => setError(String(e)));
    }
  }, [open, diffs, repoRoot]);
  // Keep the suggestion current until the user types their own message.
  useEffect(() => { if (h && !messageEdited) setMessage(h.suggestedMessage); }, [h, messageEdited]);

  async function run(label: string, op: GitOp): Promise<GitOpResult | null> {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      const r = await invoke<GitOpResult>("git_run", { repoPath: repoRoot, op });
      await refresh();
      return r;
    } catch (e) {
      setError(String(e));
      await refresh();
      return null;
    } finally {
      setBusy(null);
    }
  }

  async function save(andUpload: boolean) {
    if (!h) return;
    const all = h.files.map((f) => f.path);
    const files = all.filter((p) => !excluded.has(p));
    const r = await run("Saving…", { op: "commit", message, files: files.length === all.length ? null : files });
    if (!r) return;
    setMessageEdited(false);
    setExcluded(new Set());
    if (andUpload) await upload();
    else setNotice("Saved. Upload when you're ready.");
  }

  async function upload() {
    const r = await run("Uploading…", { op: "push" });
    if (r) setNotice(`Uploaded to ${serverName(h?.status?.remote_url ?? null)}.`);
  }

  async function getLatest() {
    const r = await run("Getting the latest…", { op: "pull" });
    if (r?.result === "pulled") {
      const o = r.outcome;
      setNotice(o.result === "up_to_date" ? "You already have the latest version."
        : o.result === "updated" ? `Got ${plural(o.commits, "new version", "new versions")} from the server.`
          : `Got ${plural(o.commits, "new version", "new versions")} from the server and put your saved versions on top.`);
      onFilesChanged();
    }
  }

  function toggleDiff(path: string) {
    setOpen(open === path ? null : path);
  }

  if (!h) return error ? <section className="history-panel"><p className="error">{error}</p></section> : null;

  if (!h.isRepo) {
    return (
      <section className="history-panel">
        <h2>Changes and upload</h2>
        <p>This folder has no change history (it isn't a git repository), so mxKeys can't keep versions or upload them.</p>
        <button onClick={() => run("Starting…", { op: "init" })} disabled={!!busy}>Start a change history here</button>
        <p className="muted small">To upload to GitHub later, create an empty repository there and link it in a git tool (git remote add origin …).</p>
        {error && <p className="error">{error}</p>}
      </section>
    );
  }

  const st = h.status!;
  const server = serverName(st.remote_url);
  const dirty = h.files.length > 0;
  const chosen = h.files.filter((f) => !excluded.has(f.path));
  const needsIdentity = !st.identity;

  return (
    <section className="history-panel">
      <div className="editor-head">
        <h2>Changes and upload</h2>
        {busy && <span className="muted">{busy}</span>}
      </div>

      <div className="history-state">
        {st.operation && <p className="warning">A git {st.operation} is in progress in this folder. Finish or cancel it in a git tool before saving here.</p>}
        {!dirty && st.ahead === 0 && st.behind === 0 && (
          <p className="ok">✓ Everything is saved{st.remote_name ? ` and uploaded to ${server}` : ""}.
            {st.last_commit && <span className="muted"> Last version: “{st.last_commit.summary}” · {ago(st.last_commit.time)}</span>}</p>
        )}
        {st.behind > 0 && (
          <div className="row">
            <span>{server} has {plural(st.behind, "newer version", "newer versions")} (for example edits made on the website).</span>
            <button onClick={getLatest} disabled={!!busy}>Get latest</button>
          </div>
        )}
        {st.ahead > 0 && (
          <div className="row">
            <span>{plural(st.ahead, "saved version isn't", "saved versions aren't")} uploaded to {server} yet.</span>
            <button className="primary" onClick={upload} disabled={!!busy || !st.remote_name}>Upload</button>
          </div>
        )}
        {!st.remote_name && <p className="muted small">This folder isn't linked to a server, so versions stay on this computer.</p>}
      </div>

      {dirty && (
        <>
          <p><strong>{plural(h.files.length, "file has", "files have")} unsaved changes</strong> <span className="muted">(since the last saved version)</span></p>
          <ul className="changed-files">
            {h.files.map((f) => (
              <ChangedFileRow key={f.path} f={f} included={!excluded.has(f.path)} open={open === f.path} diff={diffs[f.path]}
                onInclude={(on) => setExcluded((x) => { const n = new Set(x); if (on) n.delete(f.path); else n.add(f.path); return n; })}
                onToggle={() => toggleDiff(f.path)}
                onDiscard={async () => {
                  if (await confirmAction(`Throw away all unsaved changes to ${f.path}? This can't be undone.`, "Throw away")) {
                    if (await run("Undoing…", { op: "discard", files: [f.path] })) onFilesChanged();
                  }
                }}
                busy={!!busy}
              />
            ))}
          </ul>

          {needsIdentity ? (
            <form className="identity" onSubmit={async (e) => { e.preventDefault(); await run("Saving name…", { op: "set_identity", ...identity }); }}>
              <p>Versions are labelled with your name and email (as in any git history). They're saved in this folder's git settings only.</p>
              <input placeholder="Your name" value={identity.name} onChange={(e) => setIdentity({ ...identity, name: e.target.value })} />
              <input placeholder="Email (as on GitHub)" value={identity.email} onChange={(e) => setIdentity({ ...identity, email: e.target.value })} />
              <button type="submit" disabled={!identity.name.trim() || !identity.email.includes("@")}>Continue</button>
            </form>
          ) : (
            <div className="commit-box">
              <label className="field">
                Describe this version
                <textarea rows={Math.min(8, Math.max(2, message.split("\n").length))} value={message}
                  onChange={(e) => { setMessage(e.target.value); setMessageEdited(true); }} />
              </label>
              {messageEdited && <button className="link" onClick={() => setMessageEdited(false)}>Use the suggested description</button>}
              <div className="row">
                {st.remote_name && <button className="primary" disabled={!!busy || !message.trim() || chosen.length === 0} onClick={() => save(true)}>Save version and upload</button>}
                <button className={st.remote_name ? "" : "primary"} disabled={!!busy || !message.trim() || chosen.length === 0} onClick={() => save(false)}>Save version{st.remote_name ? " only" : ""}</button>
                {chosen.length < h.files.length && <span className="muted small">{plural(chosen.length, "file", "files")} of {h.files.length} included</span>}
              </div>
            </div>
          )}
        </>
      )}
      {notice && <p className="note">{notice}</p>}
      {error && <p className="error">{error}</p>}

      <details>
        <summary>Advanced</summary>
        <table className="small">
          <tbody>
            <tr><td>branch</td><td><code>{st.branch ?? "(detached HEAD)"}</code>{st.upstream && <> → <code>{st.upstream}</code></>}</td></tr>
            <tr><td>remote</td><td><code>{st.remote_name ?? "none"}</code> {st.remote_url && <code>{st.remote_url}</code>}</td></tr>
            <tr><td>git</td><td>{h.backend.kind === "system_git" ? <code>{h.backend.version}</code> : "built-in (libgit2) — install git for SSH remotes and combining diverged histories"}</td></tr>
            <tr><td>identity</td><td>{st.identity ? `${st.identity[0]} <${st.identity[1]}>` : "not set"}</td></tr>
            {fetchProblem && <tr><td>server check</td><td className="error">{fetchProblem}</td></tr>}
          </tbody>
        </table>
        {h.recent.length > 0 && (
          <>
            <p className="muted small">Recent versions</p>
            <ul className="recent-commits small">
              {h.recent.map((c) => <li key={c.id}><code>{c.short_id}</code> {c.summary} <span className="muted">· {c.author} · {ago(c.time)}</span></li>)}
            </ul>
          </>
        )}
        <button className="link" onClick={() => run("Checking…", { op: "fetch" }).then((r) => { if (r) setFetchProblem(null); })}>Check the server for new versions</button>
      </details>
    </section>
  );
}

function ChangedFileRow({ f, included, open, diff, onInclude, onToggle, onDiscard, busy }: {
  f: ChangedFile; included: boolean; open: boolean; diff?: FileDiff[];
  onInclude: (on: boolean) => void; onToggle: () => void; onDiscard: () => void; busy: boolean;
}) {
  return (
    <li className="changed-file">
      <div className="row">
        <label className="check"><input type="checkbox" checked={included} onChange={(e) => onInclude(e.target.checked)} /></label>
        <code>{f.path}</code>
        <span className={`badge kind-${f.kind}`}>{KIND_LABEL[f.kind] ?? f.kind}</span>
        <span className="spacer" />
        <button className="link" onClick={onToggle}>{open ? "Hide lines" : "Show lines"}</button>
        <button className="link danger" onClick={onDiscard} disabled={busy}>Undo changes</button>
      </div>
      {f.summary.length > 0 && <ul className="change-summary">{f.summary.map((s, i) => <li key={i}>{s}</li>)}</ul>}
      {open && (diff ? <DiffView diffs={diff} /> : <p className="muted small">Loading…</p>)}
    </li>
  );
}

function DiffView({ diffs }: { diffs: FileDiff[] }) {
  if (diffs.length === 0) return <p className="muted small">No line changes.</p>;
  return (
    <div className="diff">
      {diffs.map((d) => (
        <div key={d.path}>
          {d.binary ? <p className="muted small">Binary file — no line view.</p> : d.hunks.map((hk, i) => (
            <table key={i} className="diff-hunk">
              <tbody>
                <tr className="hunk-head"><td colSpan={3}>{hk.header}</td></tr>
                {hk.lines.map((l, j) => (
                  <tr key={j} className={l.kind}>
                    <td className="ln">{l.old_line ?? ""}</td>
                    <td className="ln">{l.new_line ?? ""}</td>
                    <td className="code"><span className="sign">{l.kind === "added" ? "+" : l.kind === "removed" ? "−" : " "}</span>{l.text}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          ))}
        </div>
      ))}
    </div>
  );
}
