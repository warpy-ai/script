use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Debug)]
pub struct Promise {
    state: Arc<Mutex<PromiseInternal>>,
}

#[derive(Debug)]
struct PromiseInternal {
    state: PromiseState,
    value: Option<Value>,
    handlers: VecDeque<PromiseHandler>,
}

#[derive(Debug, Clone)]
pub struct PromiseHandler {
    on_fulfilled: Option<Value>,
    on_rejected: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Object(HashMap<String, Value>),
    Function(Function),
    Promise(Promise),
    Array(Vec<Value>),
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub callback: Arc<dyn Fn(Vec<Value>) -> Value + Send + Sync>,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub properties: HashMap<String, Value>,
}

impl Promise {
    pub fn new(executor: &Value) -> Self {
        let internal = Arc::new(Mutex::new(PromiseInternal {
            state: PromiseState::Pending,
            value: None,
            handlers: VecDeque::new(),
        }));

        let internal_clone = internal.clone();

        if let Value::Function(func) = executor {
            let callback = func.callback.clone();
            thread::spawn(move || {
                let result = callback(vec![]);
                let mut internal = internal_clone.lock().unwrap();
                if matches!(internal.state, PromiseState::Pending) {
                    internal.state = PromiseState::Fulfilled;
                    internal.value = Some(result);
                }
            });
        }

        Self { state: internal }
    }

    pub fn then(&self, on_fulfilled: Option<Value>) -> Self {
        let mut internal = self.state.lock().unwrap();

        match internal.state {
            PromiseState::Pending => {
                internal.handlers.push_back(PromiseHandler {
                    on_fulfilled,
                    on_rejected: None,
                });
            }
            PromiseState::Fulfilled => {
                if let Some(handler) = on_fulfilled {
                    if let Value::Function(func) = handler {
                        let value = internal.value.take().unwrap_or(Value::Undefined);
                        let callback = func.callback.clone();
                        thread::spawn(move || {
                            callback(vec![value]);
                        });
                    }
                }
            }
            PromiseState::Rejected => {}
        }

        Self {
            state: self.state.clone(),
        }
    }

    pub fn catch(&self, on_rejected: Option<Value>) -> Self {
        let mut internal = self.state.lock().unwrap();

        match internal.state {
            PromiseState::Pending => {
                internal.handlers.push_back(PromiseHandler {
                    on_fulfilled: None,
                    on_rejected,
                });
            }
            PromiseState::Rejected => {
                if let Some(handler) = on_rejected {
                    if let Value::Function(func) = handler {
                        let value = internal.value.take().unwrap_or(Value::Undefined);
                        let callback = func.callback.clone();
                        thread::spawn(move || {
                            callback(vec![value]);
                        });
                    }
                }
            }
            PromiseState::Fulfilled => {}
        }

        Self {
            state: self.state.clone(),
        }
    }

    pub fn finally(&self, on_finally: Option<Value>) -> Self {
        self.then(on_finally.clone()).catch(on_finally)
    }

    pub fn get_state(&self) -> PromiseState {
        let internal = self.state.lock().unwrap();
        internal.state.clone()
    }

    pub fn get_value(&self) -> Option<Value> {
        let internal = self.state.lock().unwrap();
        internal.value.clone()
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Undefined => write!(f, "undefined"),
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Object(_) => write!(f, "[object Object]"),
            Value::Function(func) => write!(f, "[Function: {}]", func.name),
            Value::Promise(_) => write!(f, "[Promise]"),
            Value::Array(arr) => write!(
                f,
                "[{}]",
                arr.iter()
                    .map(|v| format!("{}", v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

use std::collections::HashMap;

pub fn create_promise(executor: &Value) -> Value {
    Value::Promise(Promise::new(executor))
}

pub fn promise_resolve(value: Value) -> Value {
    Value::Promise(Promise::new(&Value::Function(Function {
        name: "executor".to_string(),
        callback: Arc::new(move |_args: Vec<Value>| value.clone()),
    })))
}

pub fn promise_reject(reason: Value) -> Value {
    Value::Promise(Promise::new(&Value::Function(Function {
        name: "executor".to_string(),
        callback: Arc::new(move |_args: Vec<Value>| reason.clone()),
    })))
}

pub fn promise_then(promise: &Value, on_fulfilled: Option<Value>) -> Value {
    if let Value::Promise(p) = promise {
        Value::Promise(p.then(on_fulfilled))
    } else {
        Value::Undefined
    }
}

pub fn promise_catch(promise: &Value, on_rejected: Option<Value>) -> Value {
    if let Value::Promise(p) = promise {
        Value::Promise(p.catch(on_rejected))
    } else {
        Value::Undefined
    }
}

pub fn promise_finally(promise: &Value, on_finally: Option<Value>) -> Value {
    if let Value::Promise(p) = promise {
        Value::Promise(p.finally(on_finally))
    } else {
        Value::Undefined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_promise_creation() {
        let executor = Value::Function(Function {
            name: "test".to_string(),
            callback: Arc::new(|_args: Vec<Value>| Value::Number(42.0)),
        });

        let promise = Promise::new(&executor);
        assert_eq!(promise.get_state(), PromiseState::Pending);

        thread::sleep(Duration::from_millis(10));
        assert_eq!(promise.get_state(), PromiseState::Fulfilled);
        assert_eq!(promise.get_value(), Some(Value::Number(42.0)));
    }

    #[test]
    fn test_promise_then() {
        let executor = Value::Function(Function {
            name: "test".to_string(),
            callback: Arc::new(|_args: Vec<Value>| Value::Number(42.0)),
        });

        let promise = Promise::new(&executor);
        let then_promise = promise.then(Some(Value::Function(Function {
            name: "then_handler".to_string(),
            callback: Arc::new(|args: Vec<Value>| {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n * 2.0)
                } else {
                    Value::Undefined
                }
            }),
        })));

        thread::sleep(Duration::from_millis(10));
        assert_eq!(then_promise.get_state(), PromiseState::Fulfilled);
    }

    #[test]
    fn test_promise_catch() {
        let executor = Value::Function(Function {
            name: "test".to_string(),
            callback: Arc::new(|_args: Vec<Value>| Value::String("error".to_string())),
        });

        let promise = Promise::new(&executor);
        let catch_promise = promise.catch(Some(Value::Function(Function {
            name: "catch_handler".to_string(),
            callback: Arc::new(|args: Vec<Value>| {
                if let Some(Value::String(s)) = args.first() {
                    Value::String(format!("caught: {}", s))
                } else {
                    Value::Undefined
                }
            }),
        })));

        thread::sleep(Duration::from_millis(10));
        assert_eq!(catch_promise.get_state(), PromiseState::Fulfilled);
    }
}
