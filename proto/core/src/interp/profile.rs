//! Flat self-time profiler for the interpreter's hot paths.
//!
//! The minimal-kernel plan (openspec/changes/core-lib-stratification/design.md) re-adds builtins
//! as AST-rewrite intrinsics from the most bottlenecking paths first; this
//! is the instrument that names those paths. Attribution is by evaluated
//! head symbol (specials, builtins, and user defns alike — the profiler
//! does not care what a name lowers to) plus dyn-node variants for the
//! per-tick motion loops. Self time excludes child frames, so the table is
//! a flat profile, not a call tree.
//!
//! Off by default with a single branch per eval; `examples/profile.rs`
//! turns it on around a card run.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Default, Clone, Copy)]
struct Entry {
    count: u64,
    self_nanos: u64,
    total_nanos: u64,
}

struct State {
    entries: HashMap<String, Entry>,
    /// (child-time accumulated inside the currently open frame), one per
    /// open frame; popped self time = elapsed - child accum.
    stack: Vec<u64>,
}

thread_local! {
    static ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static STATE: RefCell<State> = RefCell::new(State {
        entries: HashMap::new(),
        stack: Vec::new(),
    });
}

pub fn set_enabled(on: bool) {
    ENABLED.with(|e| e.set(on));
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.entries.clear();
        s.stack.clear();
    });
}

#[inline]
pub fn enabled() -> bool {
    ENABLED.with(|e| e.get())
}

/// An open profiling frame; finish with `close(name, frame)`.
pub struct Frame {
    start: Instant,
}

#[inline]
pub fn open() -> Frame {
    STATE.with(|s| s.borrow_mut().stack.push(0));
    Frame { start: Instant::now() }
}

pub fn close(name: &str, frame: Frame) {
    let elapsed = frame.start.elapsed().as_nanos() as u64;
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let child = s.stack.pop().unwrap_or(0);
        if let Some(parent) = s.stack.last_mut() {
            *parent += elapsed;
        }
        // Allocate the String key only on first sight: an alloc + hash
        // insert on every close would charge the PARENT's self time (it
        // runs inside the parent's still-open window), inflating
        // recursive rows like dyn:frame far past their real work.
        if !s.entries.contains_key(name) {
            s.entries.insert(name.to_string(), Entry::default());
        }
        let e = s.entries.get_mut(name).expect("just inserted");
        e.count += 1;
        e.total_nanos += elapsed;
        e.self_nanos += elapsed.saturating_sub(child);
    });
}

/// Sorted snapshot: (name, count, self ns, total/inclusive ns), by self time.
pub fn report() -> Vec<(String, u64, u64, u64)> {
    STATE.with(|s| {
        let s = s.borrow();
        let mut rows: Vec<_> = s
            .entries
            .iter()
            .map(|(name, e)| (name.clone(), e.count, e.self_nanos, e.total_nanos))
            .collect();
        rows.sort_by(|a, b| b.2.cmp(&a.2));
        rows
    })
}
