use std::path::{Path, PathBuf};

pub fn native_path_join(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let parts: Vec<String> = args
        .iter()
        .filter_map(|v| {
            if let JsValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect();

    if parts.is_empty() {
        return JsValue::String("".to_string());
    }

    let mut result = PathBuf::new();
    for part in parts {
        let p = Path::new(&part);
        if p.is_absolute() {
            result = p.to_path_buf();
        } else if part == ".." {
            result.pop();
        } else if part != "." && !part.is_empty() {
            result.push(&part);
        }
    }

    JsValue::String(result.to_string_lossy().into_owned())
}

pub fn native_path_resolve(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let mut parts: Vec<String> = args
        .iter()
        .filter_map(|v| {
            if let JsValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect();

    if parts.is_empty() {
        return JsValue::String("".to_string());
    }

    let mut result = PathBuf::new();
    let mut absolute = false;

    for part in parts.iter().rev() {
        let p = Path::new(part);
        if p.is_absolute() {
            result = p.to_path_buf();
            absolute = true;
            break;
        } else if !part.is_empty() && part != "." {
            result.push(part);
        }
    }

    if !absolute {
        if let Ok(cwd) = std::env::current_dir() {
            let mut cwd = cwd.to_path_buf();
            for part in parts.iter() {
                if part == ".." {
                    cwd.pop();
                } else if part != "." && !part.is_empty() {
                    cwd.push(part);
                }
            }
            result = cwd;
        }
    }

    JsValue::String(result.to_string_lossy().into_owned())
}

pub fn native_path_dirname(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(p)) = args.first() {
        let path = Path::new(p);
        let dir = path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string());
        JsValue::String(dir)
    } else {
        JsValue::String(".".to_string())
    }
}

pub fn native_path_basename(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let p = match args.first() {
        Some(JsValue::String(s)) => s.clone(),
        Some(_) => "".to_string(),
        None => "".to_string(),
    };

    let ext = args.get(1).and_then(|v| {
        if let JsValue::String(s) = v {
            Some(s.clone())
        } else {
            None
        }
    });

    let path = Path::new(&p);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.clone());

    if let Some(ext) = ext {
        if file_name.ends_with(&ext) {
            let without_ext = &file_name[..file_name.len() - ext.len()];
            return JsValue::String(without_ext.to_string());
        }
    }

    JsValue::String(file_name)
}

pub fn native_path_extname(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(p)) = args.first() {
        let path = Path::new(p);
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().into_owned();
            if ext_str.is_empty() {
                return JsValue::String("".to_string());
            }
            return JsValue::String(format!(".{}", ext_str));
        }
    }
    JsValue::String("".to_string())
}

pub fn native_path_parse(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(p)) = args.first() {
        let path = Path::new(p);

        let root = if path.is_absolute() {
            if cfg!(windows) {
                if let Some(drive) = path.to_string_lossy().chars().nth(0) {
                    if path.to_string_lossy().chars().nth(1) == Some(':') {
                        if let Some(sep) = path.to_string_lossy().chars().nth(2) {
                            format!("{}:\\", drive)
                        } else {
                            format!("{}:", drive)
                        }
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                }
            } else {
                "/".to_string()
            }
        } else {
            "".to_string()
        };

        let dir = path
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| "".to_string());

        let base = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "".to_string());

        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_else(|| "".to_string());

        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "".to_string());

        let obj_ptr = vm.heap.len();
        let mut props = std::collections::HashMap::new();
        props.insert("root".to_string(), JsValue::String(root));
        props.insert("dir".to_string(), JsValue::String(dir));
        props.insert("base".to_string(), JsValue::String(base));
        props.insert("ext".to_string(), JsValue::String(ext));
        props.insert("name".to_string(), JsValue::String(name));
        vm.heap.push(vm::HeapObject {
            data: vm::HeapData::Object(props),
        });

        return JsValue::Object(obj_ptr);
    }
    JsValue::Undefined
}

pub fn native_path_format(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let obj = match args.first() {
        Some(JsValue::Object(ptr)) => *ptr,
        _ => return JsValue::String("".to_string()),
    };

    if let Some(heap_obj) = vm.heap.get(obj) {
        if let vm::HeapData::Object(props) = &heap_obj.data {
            let root = props
                .get("root")
                .and_then(|v| {
                    if let JsValue::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "".to_string());

            let dir = props
                .get("dir")
                .and_then(|v| {
                    if let JsValue::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "".to_string());

            let base = props
                .get("base")
                .and_then(|v| {
                    if let JsValue::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "".to_string());

            let name = props
                .get("name")
                .and_then(|v| {
                    if let JsValue::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "".to_string());

            let ext = props
                .get("ext")
                .and_then(|v| {
                    if let JsValue::String(s) = v {
                        Some(if s.starts_with('.') {
                            s.clone()
                        } else {
                            format!(".{}", s)
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "".to_string());

            let mut result = String::new();

            if !root.is_empty() {
                result.push_str(&root);
            }

            if !dir.is_empty() {
                if !result.is_empty() && !result.ends_with(std::path::MAIN_SEPARATOR) {
                    result.push(std::path::MAIN_SEPARATOR);
                }
                result.push_str(&dir);
            }

            if !name.is_empty() {
                if !result.is_empty() && !result.ends_with(std::path::MAIN_SEPARATOR) {
                    result.push(std::path::MAIN_SEPARATOR);
                }
                result.push_str(&name);
            }

            result.push_str(&ext);

            if !base.is_empty() {
                if !result.is_empty() && !result.ends_with(std::path::MAIN_SEPARATOR) {
                    result.push(std::path::MAIN_SEPARATOR);
                }
                result.push_str(&base);
            }

            return JsValue::String(result);
        }
    }

    JsValue::String("".to_string())
}

pub fn native_path_is_absolute(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(p)) = args.first() {
        JsValue::Boolean(Path::new(p).is_absolute())
    } else {
        JsValue::Boolean(false)
    }
}

pub fn native_path_relative(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let from = args
        .first()
        .and_then(|v| {
            if let JsValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }
        })
        .unwrap_or("");

    let to = args
        .get(1)
        .and_then(|v| {
            if let JsValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }
        })
        .unwrap_or("");

    if from.is_empty() || to.is_empty() {
        return JsValue::String(to.to_string());
    }

    let from_path = Path::new(from);
    let to_path = Path::new(to);

    if let Ok(from_abs) = from_path.canonicalize() {
        if let Ok(to_abs) = to_path.canonicalize() {
            if let Ok(rel_path) = to_abs.strip_prefix(&from_abs) {
                let rel_str = rel_path.to_string_lossy().into_owned();
                if rel_str.is_empty() {
                    return JsValue::String(".".to_string());
                }
                return JsValue::String(rel_str);
            }
        }
    }

    JsValue::String(to.to_string())
}

pub fn native_path_to_namespaced_path(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if cfg!(windows) {
        if let Some(JsValue::String(p)) = args.first() {
            if !p.starts_with("\\\\?\\") {
                return JsValue::String(format!("\\\\?\\{}", p));
            }
        }
    }
    args.first().cloned().unwrap_or(JsValue::Undefined)
}

use crate::vm::{self, JsValue, VM};
