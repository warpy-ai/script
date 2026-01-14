//! Standard Library - Native functions for the WarpyScript VM

use crate::vm::VM;
use crate::vm::value::JsValue;

/// console.log implementation
pub fn native_log(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let output: Vec<String> = args.iter().map(|arg| format!("{:?}", arg)).collect();
    println!("LOG: {}", output.join(" "));
    JsValue::Undefined
}

/// setTimeout implementation
pub fn native_set_timeout(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if !args.is_empty() {
        let callback = args[0].clone();
        let delay_ms = args
            .get(1)
            .and_then(|v| match v {
                JsValue::Number(n) => Some(*n as u64),
                _ => None,
            })
            .unwrap_or(0);

        vm.schedule_timer(callback, delay_ms);
    }
    JsValue::Undefined
}

pub fn native_read_file(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.get(0) {
        match std::fs::read_to_string(path) {
            Ok(content) => JsValue::String(content),
            Err(e) => JsValue::String(format!("Error reading file: {}", e)),
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_write_file(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.get(0) {
        let content = args
            .get(1)
            .and_then(|v| match v {
                JsValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        match std::fs::write(path, content) {
            Ok(_) => JsValue::Undefined,
            Err(e) => JsValue::String(format!("Error writing file: {}", e)),
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_require(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(module_name)) = args.get(0) {
        let module = _vm
            .modules
            .get(module_name)
            .cloned()
            .unwrap_or(JsValue::Undefined);
        return module;
    }
    JsValue::Undefined
}
