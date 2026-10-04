use std::collections::HashSet;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use crate::error::{AppError, Result};

struct State {
    working: HashSet<String>,
    child: Option<Child>,
}

pub struct SleepInhibitor {
    state: Mutex<State>,
    spawn: fn() -> Result<Child>,
}

fn spawn_caffeinate() -> Result<Child> {
    #[cfg(target_os = "macos")]
    {
        Ok(Command::new("/usr/bin/caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(AppError::Operation(anyhow::anyhow!(
            "Idle-sleep prevention is supported only on macOS"
        )))
    }
}

impl SleepInhibitor {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                working: HashSet::new(),
                child: None,
            }),
            spawn: spawn_caffeinate,
        }
    }

    #[cfg(test)]
    fn with_spawner(spawn: fn() -> Result<Child>) -> Self {
        Self {
            state: Mutex::new(State {
                working: HashSet::new(),
                child: None,
            }),
            spawn,
        }
    }

    pub fn set_working(&self, session_id: &str, working: bool) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::Operation(anyhow::anyhow!("Idle-sleep state lock poisoned")))?;
        if working {
            state.working.insert(session_id.into());
            if let Some(child) = state.child.as_mut() {
                if child.try_wait()?.is_some() {
                    if let Some(mut exited) = state.child.take() {
                        exited.wait()?;
                    }
                }
            }
            if state.child.is_none() {
                state.child = Some((self.spawn)()?);
                tracing::info!("Preventing Mac idle sleep during active Copilot work");
            }
        } else {
            state.working.remove(session_id);
            if state.working.is_empty() {
                if let Some(mut child) = state.child.take() {
                    if child.try_wait()?.is_none() {
                        child.kill()?;
                    }
                    child.wait()?;
                    tracing::info!("Released Mac idle-sleep prevention");
                }
            }
        }
        Ok(())
    }

    pub fn is_working(&self, session_id: &str) -> Result<bool> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AppError::Operation(anyhow::anyhow!("Idle-sleep state lock poisoned")))?
            .working
            .contains(session_id))
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        let Ok(mut state) = self.state.lock() else {
            tracing::error!("Could not release idle-sleep inhibitor: lock poisoned");
            return;
        };
        if let Some(mut child) = state.child.take() {
            if let Err(error) = child.kill() {
                tracing::warn!(%error, "Could not stop idle-sleep inhibitor");
            }
            if let Err(error) = child.wait() {
                tracing::warn!(%error, "Could not reap idle-sleep inhibitor");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_test_process() -> Result<Child> {
        Ok(Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?)
    }

    #[test]
    fn shares_inhibition_across_sessions_and_releases_on_last_idle() -> Result<()> {
        let guard = SleepInhibitor::with_spawner(spawn_test_process);
        guard.set_working("a", true)?;
        guard.set_working("b", true)?;
        {
            let state = guard.state.lock().expect("test state lock");
            assert_eq!(state.working.len(), 2);
            assert!(state.child.is_some());
        }
        guard.set_working("a", false)?;
        assert!(guard.state.lock().expect("test state lock").child.is_some());
        guard.set_working("b", false)?;
        assert!(guard.state.lock().expect("test state lock").child.is_none());
        Ok(())
    }

    #[test]
    fn keeps_the_turn_marked_busy_if_sleep_prevention_is_unavailable() -> Result<()> {
        fn unavailable() -> Result<Child> {
            Err(AppError::InvalidInput("caffeinate unavailable".into()))
        }
        let guard = SleepInhibitor::with_spawner(unavailable);
        assert!(guard.set_working("a", true).is_err());
        assert!(guard.is_working("a")?);
        guard.set_working("a", false)?;
        assert!(!guard.is_working("a")?);
        Ok(())
    }
}
