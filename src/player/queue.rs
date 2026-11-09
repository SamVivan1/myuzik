use crate::library::Track;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

#[derive(Debug, Clone)]
pub struct Queue {
    items: VecDeque<Track>,
    current: Option<usize>,
    repeat: RepeatMode,
    shuffle: bool,
    shuffle_order: Vec<usize>,
    shuffle_pos: usize,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
            current: None,
            repeat: RepeatMode::Off,
            shuffle: false,
            shuffle_order: Vec::new(),
            shuffle_pos: 0,
        }
    }
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn current(&self) -> Option<&Track> {
        self.current.map(|i| &self.items[i])
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn play_now(&mut self, tracks: Vec<Track>, start: usize) {
        self.items = tracks.into();
        self.current = if self.items.is_empty() {
            None
        } else {
            Some(start.min(self.items.len().saturating_sub(1)))
        };
        self.shuffle_order.clear();
        self.shuffle_pos = 0;
        if self.shuffle {
            self.rebuild_shuffle();
        }
    }

    pub fn add_to_queue(&mut self, track: Track) {
        self.items.push_back(track);
    }

    pub fn play_next(&mut self, track: Track) {
        if let Some(i) = self.current {
            self.items.insert(i + 1, track);
        } else {
            self.items.push_front(track);
            self.current = Some(0);
        }
    }

    pub fn next(&mut self) -> Option<Track> {
        if self.items.is_empty() {
            return None;
        }
        if self.repeat == RepeatMode::One {
            return self.current.map(|i| self.items[i].clone());
        }
        if self.shuffle {
            self.shuffle_pos += 1;
            if self.shuffle_pos < self.shuffle_order.len() {
                let idx = self.shuffle_order[self.shuffle_pos];
                self.current = Some(idx);
                return Some(self.items[idx].clone());
            }
            // end of shuffle
            if self.repeat == RepeatMode::All {
                self.shuffle_pos = 0;
                let idx = self.shuffle_order[0];
                self.current = Some(idx);
                return Some(self.items[idx].clone());
            }
            self.current = None;
            return None;
        }
        // linear
        if let Some(i) = self.current {
            if i + 1 < self.items.len() {
                self.current = Some(i + 1);
                return Some(self.items[i + 1].clone());
            }
            if self.repeat == RepeatMode::All {
                self.current = Some(0);
                return Some(self.items[0].clone());
            }
        }
        self.current = None;
        None
    }

    pub fn previous(&mut self) -> Option<Track> {
        if self.items.is_empty() {
            return None;
        }
        if self.shuffle {
            if self.shuffle_pos > 0 {
                self.shuffle_pos -= 1;
                let idx = self.shuffle_order[self.shuffle_pos];
                self.current = Some(idx);
                return Some(self.items[idx].clone());
            }
            return self.current.map(|i| self.items[i].clone());
        }
        if let Some(i) = self.current {
            if i > 0 {
                self.current = Some(i - 1);
                return Some(self.items[i - 1].clone());
            }
            if self.repeat == RepeatMode::All {
                self.current = Some(self.items.len() - 1);
                return Some(self.items[self.items.len() - 1].clone());
            }
        }
        self.current.map(|i| self.items[i].clone())
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn set_shuffle(&mut self, on: bool) {
        self.shuffle = on;
        if on {
            self.rebuild_shuffle();
        } else {
            self.shuffle_order.clear();
            self.shuffle_pos = 0;
        }
    }

    fn rebuild_shuffle(&mut self) {
        let n = self.items.len();
        self.shuffle_order = (0..n).collect();
        // Fisher-Yates
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        // deterministic seed from current index for testability
        let seed = self.current.unwrap_or(0) as u64;
        let mut h = DefaultHasher::new();
        seed.hash(&mut h);
        let mut rng_state = h.finish();
        for i in (1..n).rev() {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (rng_state as usize) % (i + 1);
            self.shuffle_order.swap(i, j);
        }
        // move current to front
        if let Some(cur) = self.current {
            if let Some(pos) = self.shuffle_order.iter().position(|&x| x == cur) {
                self.shuffle_order.swap(0, pos);
            }
        }
        self.shuffle_pos = 0;
    }

    pub fn tracks(&self) -> Vec<Track> {
        self.items.iter().cloned().collect()
    }
}
