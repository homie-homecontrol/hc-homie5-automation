use crate::timer_manager::TimerManager;
use config_watcher::ConfigItemHash;
use mlua::UserData;
use std::time::Duration;

pub struct LuaTimer {
    pub timers: TimerManager,
    pub rule_hash: ConfigItemHash,
}

impl UserData for LuaTimer {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("create", |_, timer, (id, duration, repeat): (String, u64, Option<u64>)| {
            timer.timers.create_timer(
                timer.rule_hash,
                id,
                Duration::from_secs(duration),
                repeat.map(Duration::from_secs),
                None,
                None,
            );
            Ok(())
        });
        methods.add_method("cancel", |_, timer, id: String| {
            timer.timers.cancel_timer(&id);

            Ok(())
        });
    }
}
