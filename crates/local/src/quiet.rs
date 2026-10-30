//! Background work at background priority (decision #114). Scans, catch-up
//! syncs, backups and alert checks run while the user works in a DAW, and
//! on the first launch after an update several of them start at once over
//! tens of thousands of files. A thread doing that work asks for
//! [`background`]: on Windows the thread's CPU, disk and memory priority all
//! drop (`THREAD_MODE_BACKGROUND_BEGIN`), elsewhere its CPU priority does,
//! so the desktop and the DAW always come first.

use std::cell::Cell;

use thread_priority::ThreadPriority;

thread_local! {
    /// Guards alive on this thread; only the outermost one changes priority.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Holds this thread at background priority until dropped.
#[must_use = "the thread goes back to normal priority when this is dropped"]
pub struct Quiet {
    /// What to put back, when this guard lowered the priority.
    restore: Option<Restore>,
}

enum Restore {
    #[cfg(windows)]
    EndBackgroundMode,
    #[cfg(not(windows))]
    Priority(ThreadPriority),
}

/// Lower this thread's priority until the guard is dropped. Best effort:
/// when the OS says no, the work runs at normal priority.
pub fn background() -> Quiet {
    let outermost = DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get() == 1
    });
    Quiet {
        restore: outermost.then(lower).flatten(),
    }
}

#[cfg(windows)]
fn lower() -> Option<Restore> {
    use thread_priority::windows::WinAPIThreadPriority;
    thread_priority::set_current_thread_priority(ThreadPriority::Os(
        WinAPIThreadPriority::BackgroundModeBegin.into(),
    ))
    .ok()
    .map(|()| Restore::EndBackgroundMode)
}

#[cfg(not(windows))]
fn lower() -> Option<Restore> {
    let before = thread_priority::get_current_thread_priority().ok()?;
    thread_priority::set_current_thread_priority(ThreadPriority::Min)
        .ok()
        .map(|()| Restore::Priority(before))
}

impl Drop for Quiet {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        match self.restore.take() {
            #[cfg(windows)]
            Some(Restore::EndBackgroundMode) => {
                use thread_priority::windows::WinAPIThreadPriority;
                let _ = thread_priority::set_current_thread_priority(ThreadPriority::Os(
                    WinAPIThreadPriority::BackgroundModeEnd.into(),
                ));
            }
            #[cfg(not(windows))]
            Some(Restore::Priority(p)) => {
                let _ = thread_priority::set_current_thread_priority(p);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_guards_lower_once_and_work_runs_either_way() {
        let before = thread_priority::get_current_thread_priority().ok();
        {
            let _outer = background();
            let inner = background();
            assert!(
                inner.restore.is_none(),
                "only the outermost guard changes priority"
            );
            drop(inner);
            assert_eq!(DEPTH.with(Cell::get), 1);
        }
        assert_eq!(DEPTH.with(Cell::get), 0);
        // Back where it started (Windows reports background mode as a priority too).
        assert_eq!(thread_priority::get_current_thread_priority().ok(), before);
    }

    #[test]
    fn a_new_thread_starts_unguarded() {
        let _g = background();
        std::thread::spawn(|| assert_eq!(DEPTH.with(Cell::get), 0))
            .join()
            .unwrap();
    }
}
