//! 表外名字（`NsId::Other/Unbound`、`LocalName::Other`、前缀）的驻留表。每个 [`crate::xml::Dom`] 一张。

use std::collections::HashMap;
use std::sync::Arc;

/// 驻留字符串的句柄。只在产生它的 [`Interner`] 内有意义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Interned(u32);

#[derive(Debug, Default, Clone)]
pub struct Interner {
    map: HashMap<Arc<str>, u32>,
    names: Vec<Arc<str>>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> Interned {
        if let Some(&id) = self.map.get(s) {
            return Interned(id);
        }
        let id = u32::try_from(self.names.len()).expect("interner overflow");
        let arc: Arc<str> = Arc::from(s);
        self.names.push(arc.clone());
        self.map.insert(arc, id);
        Interned(id)
    }

    pub fn get(&self, s: &str) -> Option<Interned> {
        self.map.get(s).map(|&id| Interned(id))
    }

    pub fn resolve(&self, id: Interned) -> &str {
        &self.names[id.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_is_idempotent_and_resolves() {
        let mut i = Interner::new();
        let a = i.intern("w16sdtfl");
        let b = i.intern("w16sdtfl");
        let c = i.intern("other");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(i.resolve(a), "w16sdtfl");
        assert_eq!(i.get("other"), Some(c));
        assert_eq!(i.get("missing"), None);
        assert_eq!(i.len(), 2);
    }
}
