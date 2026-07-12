use super::*;

impl Sim {
    pub(super) fn refresh_channels(&mut self, inputs: &Inputs) -> Result<(), String> {
        let mut ch = (*self.ctx.sig.channels).clone();
        // host channels verbatim — passed by name (§3)
        for (k, v) in &inputs.vals {
            ch.insert(k.to_string(), v.clone());
        }
        // $tick: the world clock as a channel (refreshed at step start, so
        // control-layer reads see the current tick). Absolute time is what
        // lets deadline columns be plain data — (invuln …) in the stdlib is
        // a set-col of :iframe-until = $tick + window, not an engine verb.
        ch.insert("tick".into(), Val::Num(self.world.tick as f64));
        // host-input streams claimed by (from-host :name): copy the host's
        // per-tick value into the stream; an absent value leaves the last
        // one (including a set!) standing — the producer yielded nothing.
        {
            let hosts = self.ctx.sig.host_streams.borrow().clone();
            let mut cells = self.ctx.sig.cells.borrow_mut();
            for (name, id) in hosts {
                if let Some(v) = ch.get(&name) {
                    cells.insert(id, (name, v.clone()));
                }
            }
        }
        // (defchannel $name expr) — card-defined derived channel defaults:
        // evaluated in definition order, before runtime producers.
        let rules = self.card_channels.clone();
        for (name, form) in rules.iter() {
            self.ctx.sig.channels = Rc::new(ch.clone());
            let v = evaluate(form, &Env::empty(), &mut self.ctx, &mut self.world)
                .map_err(|e| format!("defchannel ${}: {}", name, e))?;
            if !matches!(v, Val::Nothing) {
                ch.insert(name.to_string(), v);
            }
        }
        // (export cell) — pattern cells published as read-only channels
        for (name, id) in self.ctx.sig.exports.borrow().iter() {
            if let Some((_, v)) = self.ctx.sig.cells.borrow().get(id) {
                ch.insert(name.clone(), v.clone());
            }
        }
        // (bind-channel! $name expr) — instance-scoped derived channels.
        let bound = self.ctx.sig.bound_channels.borrow().clone();
        for (name, form, env) in bound.iter() {
            self.ctx.sig.channels = Rc::new(ch.clone());
            let v = evaluate(form, env, &mut self.ctx, &mut self.world)
                .map_err(|e| format!("bind-channel ${}: {}", name, e))?;
            if !matches!(v, Val::Nothing) {
                ch.insert(name.to_string(), v);
            }
        }
        // (bind! $x expr) stream producers, in attachment order (top-level
        // binds in card order, then runtime attachments). A producer
        // yielding `nothing` leaves the last value — a set! — standing.
        self.ctx.sig.channels = Rc::new(ch.clone());
        let prods = self.ctx.sig.producers.borrow().clone();
        for (id, form, env) in prods.iter() {
            let v = evaluate(form, env, &mut self.ctx, &mut self.world)
                .map_err(|e| format!("bind! stream {}: {}", id, e))?;
            // stream-valued producer mirrors its source (from-host binds)
            let v = match v {
                Val::Stream(src) => self.ctx.sig.stream_val(src).unwrap_or(Val::Nothing),
                v => v,
            };
            if !matches!(v, Val::Nothing) {
                if let Some(slot) = self.ctx.sig.cells.borrow_mut().get_mut(id) {
                    slot.1 = v;
                }
            }
        }
        // exported streams re-publish into the public snapshot after all
        // producers have run (host reads and lowered signals see the
        // end-of-refresh value; interpreted reads resolve the store live)
        for (name, id) in self.ctx.sig.exports.borrow().iter() {
            if let Some((_, v)) = self.ctx.sig.cells.borrow().get(id) {
                ch.insert(name.clone(), v.clone());
            }
        }
        self.ctx.sig.channels = Rc::new(ch);
        Ok(())
    }
}
