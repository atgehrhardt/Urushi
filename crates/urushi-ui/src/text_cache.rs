//! A bounded cache of what a piece of text gives at a font and size (its width, its
//! shaped glyphs), shared by layout and the renderer.
//!
//! It keeps two generations: when the newer one is full, the older is dropped and the
//! newer takes its place, and a hit in the older one moves the entry back into the
//! newer. So a screen that uses more text than one generation holds keeps what it used
//! last time, instead of losing everything at once and shaping it all again (UR-66,
//! ADR 0048). Lookups borrow the text, so a hit allocates nothing.

use std::collections::HashMap;

use crate::scene::Font;

/// One generation: the text's values by font and size (its bits).
type Generation<V> = HashMap<(Font, u32), HashMap<Box<str>, V>>;

/// Values computed from `(text, font, size)`, at most about twice `capacity` of them.
#[derive(Debug)]
pub struct TextCache<V> {
    newer: Generation<V>,
    older: Generation<V>,
    len: usize,
    capacity: usize,
}

impl<V> TextCache<V> {
    /// An empty cache whose generations hold `capacity` entries each.
    pub fn new(capacity: usize) -> Self {
        Self {
            newer: HashMap::new(),
            older: HashMap::new(),
            len: 0,
            capacity: capacity.max(1),
        }
    }

    /// The value for `text` at `font` and `size` (in bits), computed by `make` if
    /// neither generation has it.
    pub fn get_or_insert_with(
        &mut self,
        (text, font, size): (&str, Font, u32),
        make: impl FnOnce() -> V,
    ) -> &V {
        let fresh = self
            .newer
            .get(&(font, size))
            .is_some_and(|m| m.contains_key(text));
        if !fresh {
            let value = self
                .older
                .get_mut(&(font, size))
                .and_then(|m| m.remove(text))
                .unwrap_or_else(make);
            if self.len >= self.capacity {
                self.older = std::mem::take(&mut self.newer);
                self.len = 0;
            }
            self.newer
                .entry((font, size))
                .or_default()
                .insert(text.into(), value);
            self.len += 1;
        }
        // Just made sure it is there.
        &self.newer[&(font, size)][text]
    }

    /// How many entries the newer generation holds.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the newer generation is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Forgets everything (e.g. when what the values point into is rebuilt).
    pub fn clear(&mut self) {
        self.newer.clear();
        self.older.clear();
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_are_kept_across_a_generation_and_misses_computed_once() {
        let mut cache = TextCache::new(3);
        let mut made = 0;
        let mut get = |cache: &mut TextCache<usize>, t: &str| {
            *cache.get_or_insert_with((t, Font::Ui, 12), || {
                made += 1;
                t.len()
            })
        };
        for t in ["a", "bb", "ccc"] {
            get(&mut cache, t);
        }
        // "dddd" starts a new generation; "a" is still found in the older one.
        assert_eq!(get(&mut cache, "dddd"), 4);
        assert_eq!(get(&mut cache, "a"), 1);
        assert_eq!(get(&mut cache, "a"), 1);
        assert_eq!(made, 4);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn fonts_and_sizes_are_kept_apart() {
        let mut cache = TextCache::new(10);
        cache.get_or_insert_with(("x", Font::Ui, 12), || 1);
        assert_eq!(
            *cache.get_or_insert_with(("x", Font::UiMedium, 12), || 2),
            2
        );
        assert_eq!(*cache.get_or_insert_with(("x", Font::Ui, 13), || 3), 3);
        assert_eq!(*cache.get_or_insert_with(("x", Font::Ui, 12), || 9), 1);
        cache.clear();
        assert!(cache.is_empty());
    }
}
