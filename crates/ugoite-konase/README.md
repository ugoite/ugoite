# ugoite-konase

Portable client-side control-plane semantics for Konase.

The crate owns the Work, Job, Observation, Context Capsule, event, and effect
contracts. The step function is deterministic and performs no network,
filesystem, storage, async runtime, or model-provider work. Hosts execute
returned effects and feed the resulting events back into the state machine.

Konase state is disposable client state. Meaningful Knowledge outcomes remain
owned by Ugoite's existing Change/Run/Undo and Space boundaries; this crate
does not define a second durable history.
