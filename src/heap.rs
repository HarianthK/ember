use crate::chunk::{Function, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

// A handle to an object in the heap: an index, small and copyable. Two values holding
// the same handle hold the same object, which is what identity means for lists and maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ref(u32);

// A captured variable. Open while its scope is alive, pointing at the stack slot; closed
// when the scope ends, holding the value itself. Closures that share it see one variable.
#[derive(Debug)]
pub enum Upvalue {
    Open(usize),
    Closed(Value),
}

#[derive(Debug)]
pub struct Closure {
    pub function: Rc<Function>,
    pub upvalues: Vec<Ref>,
}

#[derive(Debug)]
pub enum Obj {
    List(Vec<Value>),
    // String keys only, as in JSON. Kept sorted, so a map always prints the same way.
    Map(BTreeMap<String, Value>),
    Closure(Closure),
    Upvalue(Upvalue),
}

// Every object the program creates lives here. A freed slot goes on the free list and is
// reused, so handles stay small; reading a freed slot panics rather than reading garbage.
#[derive(Default)]
pub struct Heap {
    slots: Vec<Option<Obj>>,
    free: Vec<u32>,
}

impl Heap {
    pub fn alloc(&mut self, obj: Obj) -> Ref {
        if let Some(i) = self.free.pop() {
            self.slots[i as usize] = Some(obj);
            return Ref(i);
        }
        self.slots.push(Some(obj));
        Ref((self.slots.len() - 1) as u32)
    }

    // How many objects are alive, which is what the collector's tests measure.
    pub fn live(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    fn get(&self, r: Ref) -> &Obj {
        self.slots[r.0 as usize]
            .as_ref()
            .expect("a handle to an object that was freed")
    }

    fn get_mut(&mut self, r: Ref) -> &mut Obj {
        self.slots[r.0 as usize]
            .as_mut()
            .expect("a handle to an object that was freed")
    }

    pub fn list(&self, r: Ref) -> &Vec<Value> {
        match self.get(r) {
            Obj::List(items) => items,
            other => unreachable!("a list handle pointing at {other:?}"),
        }
    }

    pub fn list_mut(&mut self, r: Ref) -> &mut Vec<Value> {
        match self.get_mut(r) {
            Obj::List(items) => items,
            other => unreachable!("a list handle pointing at {other:?}"),
        }
    }

    pub fn map(&self, r: Ref) -> &BTreeMap<String, Value> {
        match self.get(r) {
            Obj::Map(entries) => entries,
            other => unreachable!("a map handle pointing at {other:?}"),
        }
    }

    pub fn map_mut(&mut self, r: Ref) -> &mut BTreeMap<String, Value> {
        match self.get_mut(r) {
            Obj::Map(entries) => entries,
            other => unreachable!("a map handle pointing at {other:?}"),
        }
    }

    pub fn closure(&self, r: Ref) -> &Closure {
        match self.get(r) {
            Obj::Closure(c) => c,
            other => unreachable!("a closure handle pointing at {other:?}"),
        }
    }

    pub fn upvalue(&self, r: Ref) -> &Upvalue {
        match self.get(r) {
            Obj::Upvalue(u) => u,
            other => unreachable!("an upvalue handle pointing at {other:?}"),
        }
    }

    pub fn upvalue_mut(&mut self, r: Ref) -> &mut Upvalue {
        match self.get_mut(r) {
            Obj::Upvalue(u) => u,
            other => unreachable!("an upvalue handle pointing at {other:?}"),
        }
    }

    // A value as print shows it. Strings inside a list or map are quoted so ["1"] and [1]
    // differ, and a list or map met again while printing shows as [...] or {...}.
    pub fn show(&self, value: &Value) -> String {
        let mut out = String::new();
        self.write(&mut out, value, &mut Vec::new(), false);
        out
    }

    fn write(&self, out: &mut String, value: &Value, open: &mut Vec<Ref>, quoted: bool) {
        match value {
            Value::Str(s) if quoted => out.push_str(&format!("{s:?}")),
            Value::List(r) => {
                if open.contains(r) {
                    out.push_str("[...]");
                    return;
                }
                open.push(*r);
                out.push('[');
                for (i, item) in self.list(*r).iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.write(out, item, open, true);
                }
                out.push(']');
                open.pop();
            }
            Value::Map(r) => {
                if open.contains(r) {
                    out.push_str("{...}");
                    return;
                }
                open.push(*r);
                out.push('{');
                for (i, (key, item)) in self.map(*r).iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!("{key:?}: "));
                    self.write(out, item, open, true);
                }
                out.push('}');
                open.pop();
            }
            Value::Closure(r) => out.push_str(&format!("<fn {}>", self.closure(*r).function.name)),
            other => out.push_str(&other.to_string()),
        }
    }
}
