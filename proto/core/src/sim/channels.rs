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
        // :expose rules — entity columns published as channels; a dead or
        // absent entity reads 0, so hp gates fire (cards declare these:
        // {:expose {$some-hp :hp}})
        for (chan, handle, col) in &self.world.exposes {
            let v = self
                .world
                .find(*handle)
                .and_then(|i| self.world.col_get_at(i, col))
                .unwrap_or(0.0);
            ch.insert(chan.to_string(), Val::Num(v));
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
        self.ctx.sig.channels = Rc::new(ch);
        Ok(())
    }
}
