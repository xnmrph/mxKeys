import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { CloneProgress, GitBackend, RepoSummary, RepoUrl } from "./types";

function describe(p: CloneProgress | null): string {
  if (!p) return "";
  switch (p.stage) {
    case "connecting": return "Connecting…";
    case "receiving": return `Downloading… ${p.percent}%`;
    case "resolving": return `Unpacking… ${p.percent}%`;
    case "checking_out": return `Writing files… ${p.percent}%`;
    case "done": return "Done";
  }
}

function percent(p: CloneProgress | null): number {
  if (!p) return 0;
  if (p.stage === "receiving") return p.percent * 0.8;
  if (p.stage === "resolving" || p.stage === "checking_out") return 80 + p.percent * 0.2;
  return p.stage === "done" ? 100 : 2;
}

export function CloneForm({ onCloned }: { onCloned: (r: RepoSummary) => void }) {
  const [url, setUrl] = useState("");
  const [checked, setChecked] = useState<RepoUrl | null>(null);
  const [urlError, setUrlError] = useState<string | null>(null);
  const [parent, setParent] = useState<string | null>(null);
  const [progress, setProgress] = useState<CloneProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [backend, setBackend] = useState<GitBackend | null>(null);

  useEffect(() => {
    invoke<GitBackend>("git_backend").then(setBackend).catch(() => setBackend(null));
  }, []);

  useEffect(() => {
    if (!url.trim()) { setChecked(null); setUrlError(null); return; }
    let cancelled = false;
    invoke<RepoUrl>("check_repo_url", { url })
      .then((u) => { if (!cancelled) { setChecked(u); setUrlError(null); } })
      .catch((e) => { if (!cancelled) { setChecked(null); setUrlError(String(e)); } });
    return () => { cancelled = true; };
  }, [url]);

  async function chooseParent() {
    const dir = await open({ directory: true, title: "Where should the config be saved?" });
    if (typeof dir === "string") setParent(dir);
  }

  async function startClone() {
    if (!checked || !parent) return;
    setBusy(true);
    setError(null);
    setProgress({ stage: "connecting" });
    const unlisten = await listen<CloneProgress>("clone-progress", (e) => setProgress(e.payload));
    try {
      onCloned(await invoke<RepoSummary>("clone_config_repo", { url, parentDir: parent }));
    } catch (e) {
      setError(String(e));
      setProgress(null);
    } finally {
      unlisten();
      setBusy(false);
    }
  }

  const sep = parent?.includes("\\") ? "\\" : "/";
  return (
    <div className="clone">
      <label>
        Address of your config repository
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://github.com/you/zmk-config"
          spellCheck={false}
          disabled={busy}
        />
      </label>
      {urlError && <p className="error">{urlError}</p>}
      <div className="row">
        <button onClick={chooseParent} disabled={busy}>{parent ? "Change folder…" : "Choose where to save…"}</button>
        {parent && checked && <span className="muted">Will be saved in <code>{parent}{sep}{checked.dir_name}</code></span>}
      </div>
      <button className="primary" onClick={startClone} disabled={busy || !checked || !parent}>Download</button>
      {progress && (
        <div className="progress" role="status">
          <div className="bar"><div style={{ width: `${percent(progress)}%` }} /></div>
          <span className="muted">{describe(progress)}</span>
        </div>
      )}
      {error && <p className="error" role="alert">{error}</p>}
      <details>
        <summary>Advanced</summary>
        <p className="muted">
          {backend?.kind === "system_git"
            ? <>Using your installed git ({backend.version}), so your saved logins and SSH keys work.</>
            : <>git isn't installed, so the built-in downloader is used (https addresses only).</>}
          {checked && <> Address type: <code>{checked.kind}</code>.</>}
        </p>
      </details>
    </div>
  );
}
