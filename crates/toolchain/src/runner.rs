//! Running external programs (python, west, cmake) with streamed output, behind a trait
//! so orchestration can be tested without actually running them.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Extra/overridden environment variables (the rest is inherited).
    pub env: BTreeMap<String, String>,
}

impl Cmd {
    pub fn new(program: impl Into<PathBuf>) -> Cmd {
        Cmd {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
        }
    }
    pub fn arg(mut self, a: impl Into<String>) -> Cmd {
        self.args.push(a.into());
        self
    }
    pub fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, a: I) -> Cmd {
        self.args.extend(a.into_iter().map(Into::into));
        self
    }
    pub fn cwd(mut self, d: impl Into<PathBuf>) -> Cmd {
        self.cwd = Some(d.into());
        self
    }
    pub fn env(mut self, k: impl Into<String>, v: impl Into<String>) -> Cmd {
        self.env.insert(k.into(), v.into());
        self
    }
    pub fn envs(mut self, e: &BTreeMap<String, String>) -> Cmd {
        self.env.extend(e.iter().map(|(k, v)| (k.clone(), v.clone())));
        self
    }
    /// For logs: `program arg1 arg2` (no environment).
    pub fn display(&self) -> String {
        std::iter::once(self.program.display().to_string())
            .chain(self.args.iter().map(|a| {
                if a.contains(' ') {
                    format!("\"{a}\"")
                } else {
                    a.clone()
                }
            }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("couldn't start {program}: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error("{program} failed (exit code {code:?})")]
    Failed { program: String, code: Option<i32> },
}

pub trait CommandRunner: Send + Sync {
    /// Run to completion, calling `on_line` for every stdout/stderr line.
    fn run(&self, cmd: &Cmd, on_line: &mut dyn FnMut(&str)) -> Result<(), RunError>;
}

/// Spawns real processes.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    fn run(&self, cmd: &Cmd, on_line: &mut dyn FnMut(&str)) -> Result<(), RunError> {
        let program = cmd.program.display().to_string();
        let mut c = Command::new(&cmd.program);
        c.args(&cmd.args)
            .envs(&cmd.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(d) = &cmd.cwd {
            c.current_dir(d);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            c.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = c.spawn().map_err(|source| RunError::Spawn {
            program: program.clone(),
            source,
        })?;
        let (tx, rx) = mpsc::channel::<String>();
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            let tx = tx.clone();
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(out).split(b'\n').map_while(Result::ok) {
                    let _ = tx.send(String::from_utf8_lossy(&line).trim_end_matches('\r').to_string());
                }
            }));
        }
        if let Some(err) = child.stderr.take() {
            let tx = tx.clone();
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(err).split(b'\n').map_while(Result::ok) {
                    let _ = tx.send(String::from_utf8_lossy(&line).trim_end_matches('\r').to_string());
                }
            }));
        }
        drop(tx);
        for line in rx {
            on_line(&line);
        }
        for r in readers {
            let _ = r.join();
        }
        let status = child.wait().map_err(|source| RunError::Spawn {
            program: program.clone(),
            source,
        })?;
        if status.success() {
            Ok(())
        } else {
            Err(RunError::Failed {
                program,
                code: status.code(),
            })
        }
    }
}

/// A scripted reply: (substring of the command line, output lines, success).
pub type ScriptedReply = (String, Vec<String>, bool);

/// Records commands; replies with scripted output/results (for tests).
#[derive(Debug, Default, Clone)]
pub struct MockRunner {
    pub calls: Arc<Mutex<Vec<Cmd>>>,
    /// (substring of the command line, output lines, success)
    pub script: Arc<Mutex<Vec<ScriptedReply>>>,
}

impl MockRunner {
    pub fn on(&self, needle: &str, lines: &[&str], ok: bool) {
        self.script
            .lock()
            .unwrap()
            .push((needle.into(), lines.iter().map(|s| s.to_string()).collect(), ok));
    }
    pub fn commands(&self) -> Vec<String> {
        self.calls.lock().unwrap().iter().map(Cmd::display).collect()
    }
}

impl CommandRunner for MockRunner {
    fn run(&self, cmd: &Cmd, on_line: &mut dyn FnMut(&str)) -> Result<(), RunError> {
        self.calls.lock().unwrap().push(cmd.clone());
        let shown = cmd.display();
        let script = self.script.lock().unwrap();
        if let Some((_, lines, ok)) = script.iter().find(|(n, _, _)| shown.contains(n.as_str())) {
            for l in lines {
                on_line(l);
            }
            if !ok {
                return Err(RunError::Failed {
                    program: cmd.program.display().to_string(),
                    code: Some(1),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_process_streams_both_streams() {
        let (prog, args): (&str, Vec<&str>) = if cfg!(windows) {
            ("cmd", vec!["/C", "echo out & echo err 1>&2"])
        } else {
            ("sh", vec!["-c", "echo out; echo err >&2"])
        };
        let mut lines = Vec::new();
        ProcessRunner
            .run(&Cmd::new(prog).args(args), &mut |l| {
                lines.push(l.trim().to_string())
            })
            .unwrap();
        lines.sort();
        assert_eq!(lines, ["err", "out"]);
        let fail = if cfg!(windows) {
            Cmd::new("cmd").args(["/C", "exit 3"])
        } else {
            Cmd::new("sh").args(["-c", "exit 3"])
        };
        assert!(matches!(
            ProcessRunner.run(&fail, &mut |_| {}),
            Err(RunError::Failed { code: Some(3), .. })
        ));
        assert!(matches!(
            ProcessRunner.run(&Cmd::new("definitely-not-a-program-xyz"), &mut |_| {}),
            Err(RunError::Spawn { .. })
        ));
    }
}
