import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "./icons";
import type {
  Confidence, DetectionReport, FieldResult, HardwareProfile, Known, QuizAnswers, QuizQuestion, Source,
} from "./types";
import { Illustration } from "./illustrations";

const SOURCE_LABEL: Record<Source, string> = {
  settings: "your settings",
  build_yaml: "build.yaml",
  quiz: "your answers",
  usb: "USB",
  bootloader: "bootloader",
  live_connection: "live connection",
};

const CONFIDENCE_LABEL: Record<Confidence, string> = { strong: "sure", medium: "fairly sure", weak: "a guess" };

const NAMES: Record<string, string> = {
  corne: "Corne",
  nice_nano_v2: "nice!nano v2",
  nice_nano_v1: "nice!nano v1",
  xiao_ble: "XIAO BLE",
  nrf_micro: "nRFMicro",
  nice_view: "nice!view",
  oled128x32: "128×32 OLED",
  none: "No screen",
  left: "Left half",
  right: "Right half",
};

function knownName<T extends string>(k: Known<T> | null): string | null {
  if (!k) return null;
  return k.type === "known" ? NAMES[k.id] ?? k.id : `“${k.id}” (not recognised)`;
}

function Field<T>({ label, field, show }: { label: string; field: FieldResult<T>; show: (v: T) => string | null }) {
  const value = field.value === null ? null : show(field.value);
  return (
    <div className="field">
      <div className="field-label">{label}</div>
      <div className="field-value">{value ?? <span className="muted">not sure yet</span>}</div>
      {field.confidence && (
        <div className={`badge ${field.confidence}`} title={field.sources.map((s) => SOURCE_LABEL[s]).join(", ")}>
          {CONFIDENCE_LABEL[field.confidence]} · {field.sources.map((s) => SOURCE_LABEL[s]).join(" + ")}
        </div>
      )}
    </div>
  );
}

export function HardwareSetup({ repoRoot, onProfile }: { repoRoot: string; onProfile?: (p: HardwareProfile | null) => void }) {
  const [saved, setSaved] = useState<HardwareProfile | null | undefined>(undefined);
  const [report, setReport] = useState<DetectionReport | null>(null);
  const [quiz, setQuiz] = useState<QuizAnswers>({});
  const [questions, setQuestions] = useState<QuizQuestion[]>([]);
  const [showQuiz, setShowQuiz] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const quizRef = useRef(quiz);
  quizRef.current = quiz;

  const run = useCallback(async () => {
    try {
      const q = quizRef.current;
      const hasAnswers = Object.values(q).some((v) => v);
      setReport(await invoke<DetectionReport>("detect_hardware", { repoPath: repoRoot, quiz: hasAnswers ? q : null }));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [repoRoot]);

  useEffect(() => {
    invoke<HardwareProfile | null>("load_profile", { repoPath: repoRoot })
      .then((p) => { setSaved(p); onProfile?.(p); })
      .catch(() => setSaved(null));
    invoke<QuizQuestion[]>("quiz_questions").then(setQuestions).catch(() => {});
  }, [repoRoot, onProfile]);

  // While the setup panel is open, re-check every few seconds so a half that's put into
  // bootloader mode (double-tap reset) shows up by itself.
  const open = saved === null;
  useEffect(() => {
    if (!open) return;
    void run();
    const t = setInterval(() => void run(), 2500);
    return () => clearInterval(t);
  }, [open, run]);

  useEffect(() => { if (open) void run(); }, [quiz, open, run]);

  async function confirm() {
    const p = report?.detection.profile;
    if (!p) return;
    try {
      await invoke("save_profile", { repoPath: repoRoot, profile: p });
      setSaved(p);
      onProfile?.(p);
    } catch (e) {
      setError(String(e));
    }
  }

  if (saved === undefined) return null;

  if (saved) {
    return (
      <section className="hw-chip">
        <span className="hw-line"><Icon name="keyboard" /> <strong>{NAMES[saved.keyboard]} · {NAMES[saved.controller]} · {NAMES[saved.display]}</strong></span>
        <button onClick={() => { setSaved(null); onProfile?.(null); }}>Change</button>
      </section>
    );
  }

  const d = report?.detection;
  const answered = (id: string) => (quiz as Record<string, string | null | undefined>)[id];
  return (
    <section className="hw-setup">
      <h2>Your keyboard</h2>
      {!d ? <p className="muted">Looking at your config and connected devices…</p> : (
        <>
          <p className="summary">{d.summary}</p>
          <div className="fields">
            <Field label="Keyboard" field={d.keyboard} show={(v) => knownName(v)} />
            <Field label="Controller" field={d.controller} show={(v) => knownName(v)} />
            <Field label="Screen" field={d.display} show={(v) => NAMES[v]} />
            <Field label="Talks to the computer" field={d.central} show={(v) => NAMES[v]} />
            <Field label="Live editing" field={d.studio} show={(v) => (v ? "on" : "off")} />
          </div>
          {[d.keyboard, d.controller, d.display, d.central, d.studio].flatMap((f) => f.conflicts).map((c) => (
            <p key={c} className="warning">{c}</p>
          ))}
          {d.notes.map((n) => <p key={n.text} className={n.level === "warning" ? "warning" : "note"}>{n.text}</p>)}
          {report.unsupported.length > 0 && (
            <p className="warning">mxKeys can't build firmware for {report.unsupported.join(" or ")} yet. You can still look at your keymap.</p>
          )}
          <div className="bootloader-tip">
            <strong>Want to double-check the controller?</strong> Plug one half in with USB and quickly press its reset
            button twice. A drive appears for a few seconds{report.drives.length > 0 ? "" : " — we'll notice it automatically"}.
            {report.drives.map((dr) => (
              <div key={dr.path} className="note">Found <code>{dr.path}</code>: {dr.info.model ?? "unknown model"} ({dr.info.board_id ?? "no board id"})</div>
            ))}
          </div>
          <div className="row">
            <button className="primary" disabled={!d.profile} onClick={confirm}>Looks right</button>
            {d.ask.length === 0 && (
              <button onClick={() => setShowQuiz((s) => !s)}>{showQuiz ? "Hide questions" : "Something's wrong"}</button>
            )}
          </div>
          {(showQuiz || d.ask.length > 0) && (
            <div className="quiz">
              {questions.map((q) => (
                <fieldset key={q.id}>
                  <legend>{q.prompt}</legend>
                  <div className="options">
                    {q.options.map((o) => (
                      <label key={o.value} className={`option${answered(q.id) === o.value ? " chosen" : ""}`}>
                        <input
                          type="radio"
                          name={q.id}
                          checked={answered(q.id) === o.value}
                          onChange={() => setQuiz({ ...quiz, [q.id]: o.value })}
                        />
                        <Illustration value={o.value} />
                        <span className="option-label">{o.label}{!o.supported && <span className="muted"> (not supported yet)</span>}</span>
                        <span className="option-desc muted">{o.looks_like}</span>
                      </label>
                    ))}
                    <label className={`option${answered(q.id) ? "" : " chosen"}`}>
                      <input type="radio" name={q.id} checked={!answered(q.id)} onChange={() => setQuiz({ ...quiz, [q.id]: null })} />
                      <span className="option-label">I'm not sure</span>
                    </label>
                  </div>
                </fieldset>
              ))}
            </div>
          )}
          <details>
            <summary>Advanced</summary>
            <table>
              <thead><tr><th>what</th><th>from</th><th>how sure</th><th>because</th></tr></thead>
              <tbody>
                {[d.keyboard, d.controller, d.display, d.central, d.studio].flatMap((f) => f.evidence).map((e, i) => (
                  <tr key={i}>
                    <td><code>{e.fact.field}={JSON.stringify(e.fact.value)}</code></td>
                    <td>{SOURCE_LABEL[e.source]}</td>
                    <td>{e.confidence}</td>
                    <td>{e.because}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {report.targets.length > 0 && (
              <>
                <p>Firmware the app will build:</p>
                <table>
                  <thead><tr><th>target</th><th>board</th><th>shield</th><th>snippet</th></tr></thead>
                  <tbody>
                    {report.targets.map((t) => (
                      <tr key={t.id}><td>{t.id}</td><td><code>{t.board}</code></td><td><code>{t.shields.join(" ")}</code></td><td><code>{t.snippets.join(" ")}</code></td></tr>
                    ))}
                  </tbody>
                </table>
              </>
            )}
            {report.probeErrors.map((e) => <p key={e} className="muted">{e}</p>)}
          </details>
        </>
      )}
      {error && <p className="error">{error}</p>}
    </section>
  );
}
