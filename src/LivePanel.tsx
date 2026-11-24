import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirmAction } from "./dialogs";
import { Icon } from "./icons";

interface SerialCandidate { path: string; product: string | null; serial_number: string | null }
interface BleCandidate { id: string; name: string; has_studio: boolean }
interface LiveStatus { name: string; serial: string; unlocked: boolean; unsaved: boolean; ambiguous: string[] }
interface LiveView { connected: boolean; status: LiveStatus | null; via: string | null; unlockKey: [string, number] | null; offerClear: boolean }
interface KeyDiff { layer: number; layerTitle: string; position: number; fileText: string; deviceText: string }
interface Comparison { structure: string | null; diffs: KeyDiff[]; names: [number, string, string][]; unchecked: { layer: number; position: number; fileText: string; reason: string }[] }
type Via = { kind: "usb"; path: string } | { kind: "ble"; id: string } | { kind: "sim" };

/** Live editing over ZMK Studio. `onLive(true)` when key edits should go to the keyboard. */
export interface LiveInfo {
  connected: boolean;
  /** Connected and unlocked: edits go to the keyboard. */
  ready: boolean;
  /** Connected, waiting for the unlock key. */
  locked: boolean;
  name: string | null;
  /** "layer “System”, key 18" — where the unlock key is. */
  unlockHint: string | null;
  unsaved: boolean;
}

export function LivePanel({ repoRoot, keymapPath, revision, onStatus, flash, onFileChanged }: {
  repoRoot: string; keymapPath: string;
  /** Changes whenever the keymap file was (re)loaded: compare again. */
  revision: unknown;
  onStatus: (s: LiveInfo) => void;
  /** Bumped when an edit was refused because the keyboard is locked: draw attention. */
  flash: number;
  /** The keymap file changed (discard, resolving differences): reload it. */
  onFileChanged: () => Promise<void> | void;
}) {
  const [view, setView] = useState<LiveView | null>(null);
  const [usb, setUsb] = useState<SerialCandidate[]>([]);
  const [sim, setSim] = useState(false);
  const [ble, setBle] = useState<BleCandidate[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cmp, setCmp] = useState<Comparison | null>(null);

  const ready = !!view?.connected && !!view.status?.unlocked;
  const locked = !!view?.connected && !view.status?.unlocked;
  const unlockHint = view?.unlockKey ? `layer “${view.unlockKey[0]}”, key ${view.unlockKey[1]}` : null;
  const name = view?.status?.name ?? null;
  const unsaved = !!view?.status?.unsaved;
  useEffect(() => {
    onStatus({ connected: !!view?.connected, ready, locked, name, unlockHint, unsaved });
  }, [view?.connected, ready, locked, name, unlockHint, unsaved, onStatus]);
  const [flashing, setFlashing] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!flash) return;
    boxRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    setFlashing(false);
    const start = requestAnimationFrame(() => setFlashing(true));
    const t = setTimeout(() => setFlashing(false), 1600);
    return () => { cancelAnimationFrame(start); clearTimeout(t); };
  }, [flash]);

  const refresh = useCallback(async () => {
    try {
      const v = await invoke<LiveView>("live_view", { repoPath: repoRoot });
      setView(v);
      if (!v.connected) {
        const d = await invoke<{ usb: SerialCandidate[]; sim: boolean }>("live_devices");
        setUsb(d.usb);
        setSim(d.sim);
      }
    } catch (e) {
      setError(String(e));
      setView({ connected: false, status: null, via: null, unlockKey: null, offerClear: false });
    }
  }, [repoRoot]);

  useEffect(() => {
    void refresh();
    const t = setInterval(refresh, 1500);
    return () => clearInterval(t);
  }, [refresh]);

  const compare = useCallback(async () => {
    try { setCmp(await invoke<Comparison>("live_compare")); setError(null); } catch (e) { setError(String(e)); }
  }, []);
  // Compare once the keyboard is unlocked, and again whenever the file changes.
  useEffect(() => { if (ready) void compare(); else setCmp(null); }, [ready, compare, revision]);

  async function run(label: string, cmd: string, args: Record<string, unknown> = {}, fileChanges = false) {
    setBusy(label);
    setError(null);
    try {
      await invoke(cmd, args);
      if (fileChanges) await onFileChanged();
      await refresh();
      // Comparing needs the keyboard unlocked; right after connecting, the effect
      // above compares once the unlock key is pressed.
      if (ready && cmd !== "live_connect" && cmd !== "live_disconnect") await compare();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function connect(via: Via) {
    await run("Connecting…", "live_connect", { repoPath: repoRoot, keymapPath, via });
  }

  async function scan() {
    setScanning(true);
    setError(null);
    try { setBle(await invoke<BleCandidate[]>("live_scan_ble", { seconds: 5 })); } catch (e) { setError(String(e)); } finally { setScanning(false); }
  }

  if (!view) return null;
  const st = view.status;

  if (!view.connected) {
    return (
      <div className="live-panel">
        <p className="muted">Change keys on the keyboard instantly — no build or install. Your files are updated at the same time.</p>
        {(
          <div className="live-connect">
            <p className="small">Connect the <strong>left half</strong> (it runs ZMK Studio) by USB{ble === null ? ", or look for it over Bluetooth" : ""}.</p>
            {usb.length === 0 ? <p className="muted small">No keyboard found on USB yet.</p> : (
              <ul>{usb.map((u) => (
                <li key={u.path}><Icon name="usb" /> <strong>{u.product ?? "ZMK keyboard"}</strong> <span className="mono muted">{u.path}</span><span className="spacer" /><button className="primary" disabled={!!busy} onClick={() => connect({ kind: "usb", path: u.path })}>Connect</button></li>
              ))}</ul>
            )}
            {sim && <div className="row"><button onClick={() => connect({ kind: "sim" })} disabled={!!busy}>Connect simulated keyboard</button><span className="muted small">developer mode</span></div>}
            <div className="row">
              <button onClick={scan} disabled={scanning || !!busy}>{scanning ? "Looking over Bluetooth…" : "Look over Bluetooth"}</button>
              <span className="muted small">The keyboard must be paired with this computer.</span>
            </div>
            {ble && (ble.length === 0 ? <p className="muted small">Nothing found over Bluetooth.</p> : (
              <ul>{ble.map((b) => (
                <li key={b.id}><Icon name="bluetooth" /> <strong>{b.name}</strong> {b.has_studio && <span className="badge strong">Studio</span>}<span className="spacer" /><button disabled={!!busy} onClick={() => connect({ kind: "ble", id: b.id })}>Connect</button></li>
              ))}</ul>
            ))}
          </div>
        )}
        {busy && <p className="muted small">{busy}</p>}
        {error && <p className="error small">{error}</p>}
      </div>
    );
  }

  const outOfSync = cmp && (cmp.structure || cmp.diffs.length > 0 || cmp.names.length > 0);
  const allKeys = cmp?.diffs.map((d) => ({ layer: d.layer, position: d.position })) ?? [];
  return (
    <div ref={boxRef} className={`live-panel connected${ready ? " ready" : " locked"}${flashing ? " flash" : ""}`}>
      <div className="live-head">
        <span className={`dot ${ready ? "on" : "wait"}`} />
        <strong>{st?.name}</strong>
        <span className="muted small">via {view.via}</span>
        {ready ? <span className="small">Live — key changes go to the keyboard instantly.</span>
          : <span className="locked-text"><Icon name="lock" size={18} /><span>Locked — press the keyboard's <strong>Studio unlock</strong> key{view.unlockKey && <> (layer “{view.unlockKey[0]}”, key {view.unlockKey[1]})</>} to edit.</span></span>}
        <span className="spacer" />
        {ready && <>
          <button className="primary" disabled={!st?.unsaved || !!busy} onClick={() => run("Saving…", "live_save")} title="Keep the changes on the keyboard after it restarts">Save on keyboard</button>
          <button disabled={!st?.unsaved || !!busy} onClick={async () => {
            if (await confirmAction("Undo all live changes since the last save — on the keyboard and in your keymap file?", "Discard")) {
              await run("Discarding…", "live_discard", {}, true);
            }
          }}>Discard</button>
        </>}
        <button onClick={() => run("Disconnecting…", "live_disconnect")}>Disconnect</button>
      </div>
      {ready && st?.unsaved && <p className="muted small">Not saved on the keyboard yet: it forgets these changes when it restarts (they're already in your files and in the next build).</p>}
      {view.offerClear && ready && (
        <div className="warning small">
          The keyboard still has live changes saved from before the last firmware install. They would hide later edits in your files.
          {" "}<button onClick={() => run("Clearing…", "live_clear_saved")}>Clear saved live changes</button>
          <span className="muted"> (Bluetooth pairings stay.)</span>
        </div>
      )}
      {cmp?.structure && <p className="warning small">The keyboard runs different firmware than these files ({cmp.structure}). Build and install the firmware first; live editing is off until then.</p>}
      {cmp && !cmp.structure && outOfSync && (
        <div className="live-diffs">
          <p className="small"><strong>The keyboard and your files differ</strong> ({cmp.diffs.length} key{cmp.diffs.length === 1 ? "" : "s"}{cmp.names.length ? `, ${cmp.names.length} layer name${cmp.names.length === 1 ? "" : "s"}` : ""}) — for example after using ZMK Studio elsewhere, or editing files without installing.</p>
          <table className="small">
            <thead><tr><th>where</th><th>files</th><th>keyboard</th><th></th></tr></thead>
            <tbody>
              {cmp.diffs.map((d) => (
                <tr key={`${d.layer}-${d.position}`}>
                  <td>{d.layerTitle}, key {d.position + 1}</td>
                  <td><code>{d.fileText}</code></td>
                  <td><code>{d.deviceText}</code></td>
                  <td>
                    <button className="link" onClick={() => run("Writing…", "live_resolve", { keys: [{ layer: d.layer, position: d.position }], keepKeyboard: true }, true)}>Keep keyboard's</button>
                    <button className="link" onClick={() => run("Sending…", "live_resolve", { keys: [{ layer: d.layer, position: d.position }], keepKeyboard: false })}>Send files'</button>
                  </td>
                </tr>
              ))}
              {cmp.names.map(([l, f, k]) => <tr key={`n${l}`}><td>layer {l} name</td><td>{f || <i>none</i>}</td><td>{k || <i>none</i>}</td><td className="muted">rename in the editor to align</td></tr>)}
            </tbody>
          </table>
          {cmp.diffs.length > 1 && (
            <div className="row">
              <button onClick={() => run("Writing…", "live_resolve", { keys: allKeys, keepKeyboard: true }, true)}>Keep all keyboard's keys (write into files)</button>
              <button onClick={() => run("Sending…", "live_resolve", { keys: allKeys, keepKeyboard: false })}>Send all files' keys to the keyboard</button>
            </div>
          )}
        </div>
      )}
      {cmp && cmp.unchecked.length > 0 && <p className="muted small">{cmp.unchecked.length} key(s) can't be checked or changed live (e.g. {cmp.unchecked[0].fileText}: {cmp.unchecked[0].reason}).</p>}
      {st && st.ambiguous.length > 0 && <p className="muted small">Behaviors with the same name on the keyboard ({st.ambiguous.join(", ")}) can't be changed live — give them a <code>display-name</code>.</p>}
      {busy && <p className="muted small">{busy}</p>}
      {error && <p className="error small">{error}</p>}
    </div>
  );
}
