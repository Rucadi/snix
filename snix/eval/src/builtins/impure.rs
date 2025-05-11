use builtin_macros::builtins;
use genawaiter::rc::Gen;
use std::borrow::Cow;
use nu_json_api_rs::{ evaluate_command, eval_result_to_json};

use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    self as snix_eval,
    errors::ErrorKind,
    value::NixAttrs,
    vm::generators::{self, GenCo},
    NixString, Value,
};

#[builtins]
mod impure_builtins {
    use std::ffi::OsStr;
    use std::collections::HashMap;


    use super::*;
    use crate::builtins::{coerce_value_to_path, hash::hash_nix_string};
    use os_str_bytes::OsStrBytes;
    use bstr::ByteSlice;
    #[builtin("getEnv")]
    async fn builtin_get_env(co: GenCo, var: Value) -> Result<Value, ErrorKind> {
        let var_string = var.to_str()?;
        let var_bytes = var_string.as_bytes();
        let var_os: Cow<'_, OsStr> = OsStr::assert_from_raw_bytes(var_bytes);
        Ok(env::var(var_os)
            .unwrap_or_else(|_| "".into())
            .into())
    }

    #[builtin("hashFile")]
    async fn builtin_hash_file(co: GenCo, algo: Value, path: Value) -> Result<Value, ErrorKind> {
        let path = match coerce_value_to_path(&co, path).await? {
            Err(cek) => return Ok(Value::from(cek)),
            Ok(p) => p,
        };
        let r = generators::request_open_file(&co, path).await;
        hash_nix_string(algo.to_str()?, r).map(Value::from)
    }

    #[builtin("pathExists")]
    async fn builtin_path_exists(co: GenCo, path: Value) -> Result<Value, ErrorKind> {
        match coerce_value_to_path(&co, path).await? {
            Err(cek) => Ok(Value::from(cek)),
            Ok(path) => Ok(generators::request_path_exists(&co, path).await),
        }
    }

    #[builtin("readDir")]
    async fn builtin_read_dir(co: GenCo, path: Value) -> Result<Value, ErrorKind> {
        match coerce_value_to_path(&co, path).await? {
            Err(cek) => Ok(Value::from(cek)),
            Ok(path) => {
                let dir = generators::request_read_dir(&co, path).await;
                let res = dir.into_iter().map(|(name, ftype)| {
                    (
                        // TODO: propagate Vec<u8> or bytes::Bytes into NixString.
                        NixString::from(
                            String::from_utf8(name.to_vec()).expect("parsing file name as string"),
                        ),
                        Value::from(ftype.to_string()),
                    )
                });

                Ok(Value::attrs(NixAttrs::from_iter(res)))
            }
        }
    }

    #[builtin("readFile")]
    async fn builtin_read_file(co: GenCo, path: Value) -> Result<Value, ErrorKind> {
        match coerce_value_to_path(&co, path).await? {
            Err(cek) => Ok(Value::from(cek)),
            Ok(path) => {
                let mut buf = Vec::new();
                generators::request_open_file(&co, path)
                    .await
                    .read_to_end(&mut buf)?;
                Ok(Value::from(buf))
            }
        }
    }

    #[builtin("readFileType")]
    async fn builtin_read_file_type(co: GenCo, path: Value) -> Result<Value, ErrorKind> {
        match coerce_value_to_path(&co, path).await? {
            Err(cek) => Ok(Value::from(cek)),
            Ok(path) => Ok(Value::from(
                generators::request_read_file_type(&co, path)
                    .await
                    .to_string(),
            )),
        }
    }

    #[builtin("nushell")]
     async fn builtin_nushell(co: GenCo, nuscript: Value) -> Result<Value, ErrorKind> {
        match nuscript {
            Value::String(s) => {
                let mut envars: HashMap<String, String> = HashMap::new();
                
                //cloen envars from the current process
                for (key, value) in env::vars() {
                    envars.insert(key, value);
                }

                let evaluated = evaluate_command(
                    s.to_str().unwrap(),
                    envars
                );
                
                let as_json = eval_result_to_json(&evaluated);
                
                Ok(Value::String(NixString::from(as_json.to_string())))
            }
            _ => Err(ErrorKind::TypeError {
                expected: "string",
                actual: "not string",
            }),
        }
    }
}

/// Return all impure builtins, that is all builtins which may perform I/O
/// outside of the VM and so cannot be used in all contexts (e.g. WASM).
pub fn impure_builtins() -> Vec<(&'static str, Value)> {
    let mut result = impure_builtins::builtins();

    // currentTime pins the time at which evaluation was started
    {
        let seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(dur) => dur.as_secs() as i64,

            // This case is hit if the system time is *before* epoch.
            Err(err) => -(err.duration().as_secs() as i64),
        };

        result.push(("currentTime", Value::Integer(seconds)));
    }

    result
}
