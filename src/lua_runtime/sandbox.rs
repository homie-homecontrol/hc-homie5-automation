//! Lua environment limits that keep scripts from terminating or exhausting the controller.

use std::collections::HashSet;

use hc_homie5::logging::{truncate, PAYLOAD_MAX};
use mlua::{Lua, Value, Variadic};

/// Default memory limit for one script VM (64 MiB).
pub const DEFAULT_LUA_MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// Maximum nesting depth of a Lua table converted to JSON or a Homie value. Deeper tables would
/// overflow the native stack during serialization, which aborts the process.
pub const MAX_TABLE_NESTING: usize = 64;

/// Removes everything that can end the process, start programs, touch the file system, or load
/// native or precompiled code. `os` keeps only its time functions; `load` accepts text chunks
/// only, because crafted bytecode can corrupt the VM. Modules load through the configured Lua
/// module loader, never from disk.
const SANDBOX_PRELUDE: &str = r##"
os = { time = os.time, date = os.date, clock = os.clock, difftime = os.difftime }
io = nil
dofile = nil
loadfile = nil
package.loadlib = nil
package.searchpath = nil
package.path = ""
package.cpath = ""
local text_load = load
load = function(chunk, chunkname, _mode, ...)
  if select("#", ...) > 0 then
    return text_load(chunk, chunkname, "t", ...)
  end
  return text_load(chunk, chunkname, "t")
end
"##;

/// Creates a Lua VM for one script execution with the sandbox applied. `memory_limit` is in
/// bytes; `0` disables the limit.
pub fn new_sandboxed_lua(memory_limit: usize) -> mlua::Result<Lua> {
    let lua = Lua::new();
    lua.load(SANDBOX_PRELUDE).set_name("=sandbox").exec()?;
    lua.set_memory_limit(memory_limit)?;
    Ok(lua)
}

/// Replaces Lua's `print`, which writes to stdout, with a DEBUG `lua print` event that carries the
/// rule name.
pub fn install_print(lua: &Lua, rule: &str) -> mlua::Result<()> {
    let rule = rule.to_owned();
    let print = lua.create_function(move |lua, args: Variadic<Value>| {
        let message = print_message(lua, args)?;
        tracing::debug!(rule = %rule, text = truncate(&message, PAYLOAD_MAX), "lua print");
        Ok(())
    })?;
    lua.globals().set("print", print)
}

/// Converts the arguments with `tostring` and joins them with tabs, as Lua's `print` does.
fn print_message(lua: &Lua, args: Variadic<Value>) -> mlua::Result<String> {
    let tostring: mlua::Function = lua.globals().get("tostring")?;
    let mut message = String::new();
    for (index, arg) in args.into_iter().enumerate() {
        if index > 0 {
            message.push('\t');
        }
        message.push_str(&tostring.call::<mlua::String>(arg)?.to_string_lossy());
    }
    Ok(message)
}

/// Fails if `value` contains tables nested deeper than [`MAX_TABLE_NESTING`].
///
/// The walk is iterative and visits each table once, so it is safe for cyclic tables; the
/// serializer reports those separately.
pub fn check_nesting(value: &Value) -> mlua::Result<()> {
    let Value::Table(root) = value else {
        return Ok(());
    };
    let mut pending = vec![(root.clone(), 1usize)];
    let mut visited = HashSet::new();
    while let Some((table, depth)) = pending.pop() {
        if depth > MAX_TABLE_NESTING {
            return Err(mlua::Error::RuntimeError(format!(
                "table nesting exceeds the limit of {MAX_TABLE_NESTING} levels"
            )));
        }
        if !visited.insert(table.to_pointer() as usize) {
            continue;
        }
        for pair in table.pairs::<Value, Value>() {
            let (key, value) = pair?;
            for child in [key, value] {
                if let Value::Table(child) = child {
                    pending.push((child, depth + 1));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(lua: &Lua, code: &str) -> mlua::Result<Value> {
        lua.load(code).eval::<Value>()
    }

    fn eval_str(lua: &Lua, code: &str) -> String {
        match eval(lua, code) {
            Ok(Value::String(s)) => s.to_string_lossy(),
            Ok(other) => format!("{other:?}"),
            Err(err) => format!("error: {err}"),
        }
    }

    #[test]
    fn process_and_file_access_is_removed() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        for global in [
            "os.exit",
            "os.execute",
            "os.remove",
            "os.rename",
            "os.getenv",
            "os.tmpname",
            "io",
            "dofile",
            "loadfile",
            "package.loadlib",
            "package.searchpath",
        ] {
            assert_eq!(eval_str(&lua, &format!("return type({global})")), "nil", "{global}");
        }
        assert!(eval(&lua, "os.exit(1)").is_err());
    }

    #[test]
    fn print_formats_like_lua_print() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        let format = lua.create_function(print_message).unwrap();
        lua.globals().set("format_print", format).unwrap();
        assert_eq!(eval_str(&lua, "return format_print('state:', false, 1, nil)"), "state:\tfalse\t1\tnil");
        assert_eq!(eval_str(&lua, "return format_print()"), "");
    }

    #[test]
    fn installed_print_does_not_fail() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        install_print(&lua, "rule").unwrap();
        assert!(eval(&lua, "print('a', {}, 1.5)").is_ok());
    }

    #[test]
    fn time_functions_remain() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        assert_eq!(eval_str(&lua, "return type(os.time())"), "number");
        assert_eq!(eval_str(&lua, "return os.date('!%Y', 0)"), "1970");
        assert_eq!(eval_str(&lua, "return type(os.clock())"), "number");
        assert_eq!(eval_str(&lua, "return tostring(os.difftime(10, 4))"), "6.0");
    }

    #[test]
    fn load_rejects_bytecode_and_keeps_text_chunks() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        let result = eval_str(
            &lua,
            "local f, err = load(string.dump(function() return 1 end)); return tostring(f) .. ' ' .. tostring(err)",
        );
        assert!(result.starts_with("nil "), "binary chunk must be rejected, got {result}");
        assert_eq!(eval_str(&lua, "x = 5; return tostring(load('return x')())"), "5");
        assert_eq!(eval_str(&lua, "return tostring(load('return x', 'c', 'b', { x = 7 })())"), "7");
    }

    #[test]
    fn modules_are_not_loaded_from_disk() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        assert_eq!(eval_str(&lua, "return package.path .. '|' .. package.cpath"), "|");
        assert!(eval(&lua, "require('some_module_on_disk')").is_err());
    }

    #[test]
    fn memory_limit_raises_error_and_vm_stays_usable() {
        let lua = new_sandboxed_lua(8 * 1024 * 1024).unwrap();
        let err = eval(&lua, "local s = string.rep('x', 64 * 1024 * 1024); return #s").unwrap_err();
        assert!(err.to_string().contains("not enough memory"), "{err}");
        let err = eval(&lua, "local t = {}; for i = 1, 1e8 do t[i] = i end").unwrap_err();
        assert!(err.to_string().contains("not enough memory"), "{err}");
        assert_eq!(eval_str(&lua, "return tostring(1 + 1)"), "2");
    }

    #[test]
    fn nesting_check_limits_depth() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        let nested = |depth: usize| {
            eval(&lua, &format!("local t = {{}}; local c = t; for i = 2, {depth} do c.x = {{}}; c = c.x end; return t"))
                .unwrap()
        };
        assert!(check_nesting(&nested(MAX_TABLE_NESTING)).is_ok());
        assert!(check_nesting(&nested(MAX_TABLE_NESTING + 1)).is_err());
        assert!(check_nesting(&nested(100_000)).is_err());
        assert!(check_nesting(&Value::Integer(1)).is_ok());
    }

    #[test]
    fn nesting_check_counts_table_keys_and_tolerates_cycles() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        let cyclic = eval(&lua, "local t = { a = {} }; t.a.parent = t; t.self = t; return t").unwrap();
        assert!(check_nesting(&cyclic).is_ok());
        let deep_keys =
            eval(&lua, "local t = {}; local c = t; for i = 1, 100 do local k = {}; c[k] = true; c = k end; return t")
                .unwrap();
        assert!(check_nesting(&deep_keys).is_err());
    }
}
