use crate::library::Track;
use std::collections::VecDeque;

/// Upper bound on queued tracks. Autoplay radio keeps appending, so played
/// items are dropped from the front once the queue grows past this; without a
/// cap a long session would grow without limit.
pub const MAX_QUEUE: usize = 500;

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
        if self.shuffle {
            self.rebuild_shuffle();
        }
    }

    #[allow(clippy::should_implement_trait)]
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
            if self.current.is_none() {
                // The shuffle round ended (no current track); restart it on the
                // last track of the order instead of staying dead.
                let last = self.shuffle_order.len().checked_sub(1)?;
                let idx = self.shuffle_order[last];
                self.shuffle_pos = last;
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
            return Some(self.items[i].clone());
        }
        // The queue ran to its end and cleared its position; Previous is the
        // most natural thing to press there, so restart on the last track.
        let last = self.items.len() - 1;
        self.current = Some(last);
        Some(self.items[last].clone())
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
        if let Some(cur) = self.current
            && let Some(pos) = self.shuffle_order.iter().position(|&x| x == cur)
        {
            self.shuffle_order.swap(0, pos);
        }
        self.shuffle_pos = 0;
    }

    pub fn tracks(&self) -> Vec<Track> {
        self.items.iter().cloned().collect()
    }

    /// Index of the track currently playing, if any.
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// Jump to an existing queue item. Keeps shuffle valid by rebuilding its
    /// order around the new current track.
    pub fn set_current(&mut self, index: usize) -> Option<Track> {
        if index >= self.items.len() {
            return None;
        }
        self.current = Some(index);
        if self.shuffle {
            self.rebuild_shuffle();
        }
        self.items.get(index).cloned()
    }

    /// Remove a future or past item. Removing the current track is refused so
    /// the caller can decide whether to skip first; `None` also covers an
    /// out-of-range index.
    pub fn remove(&mut self, index: usize) -> Option<Track> {
        if index >= self.items.len() || self.current == Some(index) {
            return None;
        }
        let removed = self.items.remove(index)?;
        if let Some(current) = self.current
            && index < current
        {
            self.current = Some(current - 1);
        }
        if self.shuffle {
            self.rebuild_shuffle();
        }
        Some(removed)
    }

    /// Move an item within the queue. The current track follows its item if it
    /// is the one being moved.
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return false;
        }
        let Some(item) = self.items.remove(from) else {
            return false;
        };
        self.items.insert(to, item);
        if let Some(current) = self.current {
            self.current = Some(if current == from {
                to
            } else if from < current && to >= current {
                current - 1
            } else if from > current && to <= current {
                current + 1
            } else {
                current
            });
        }
        if self.shuffle {
            self.rebuild_shuffle();
        }
        true
    }

    /// Drop everything after the current track. Playback and history stay.
    pub fn clear_upcoming(&mut self) -> usize {
        let Some(current) = self.current else {
            return 0;
        };
        let removed = self.items.len().saturating_sub(current + 1);
        self.items.truncate(current + 1);
        if self.shuffle {
            self.rebuild_shuffle();
        }
        removed
    }

    /// How many tracks would play before the queue runs out. Zero when the
    /// queue is empty or already exhausted (no current position).
    pub fn remaining(&self) -> usize {
        match self.current {
            Some(i) => self.items.len().saturating_sub(i + 1),
            None => 0,
        }
    }

    /// Whether a `next()` would produce another track instead of running out.
    /// Mirrors the branches of [`Queue::next`], including shuffle and repeat.
    pub fn can_go_next(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        if self.repeat == RepeatMode::One {
            return self.current.is_some();
        }
        if self.shuffle {
            if self.shuffle_order.is_empty() {
                return false;
            }
            return self.shuffle_pos + 1 < self.shuffle_order.len()
                || self.repeat == RepeatMode::All;
        }
        match self.current {
            Some(i) => i + 1 < self.items.len() || self.repeat == RepeatMode::All,
            // `next()` gives up once the position was cleared; autoplay, not
            // the queue, is what resumes playback there.
            None => false,
        }
    }

    /// Whether a `previous()` would produce a track. True for any non-empty
    /// queue: once the queue has run out, `previous` restarts the last track.
    pub fn can_go_previous(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        if self.shuffle {
            // The shuffle order is the way back; without one there is only the
            // current track to repeat.
            return self.current.is_some() || !self.shuffle_order.is_empty();
        }
        true
    }

    /// Ids currently in the queue, for deduplicating what autoplay adds.
    pub fn ids(&self) -> std::collections::HashSet<String> {
        self.items.iter().map(|t| t.id.clone()).collect()
    }

    /// Append tracks without moving the current position. Used by the autoplay
    /// prefetch, which extends the queue while something is still playing.
    pub fn append(&mut self, tracks: Vec<Track>) {
        for track in tracks {
            self.items.push_back(track);
        }
    }

    /// Append tracks and move the current position onto the first new one,
    /// returning it. This is what resumes playback when the queue had already
    /// run out: at that point `current` is `None`, and plain `next()` would
    /// return `None` forever even though the queue is no longer empty.
    pub fn append_continue(&mut self, tracks: Vec<Track>) -> Option<Track> {
        let start = self.items.len();
        self.append(tracks);
        if self.items.len() == start {
            return None;
        }
        self.current = Some(start);
        if self.shuffle {
            // Indices changed; rebuild so the shuffle order stays valid.
            self.rebuild_shuffle();
        }
        Some(self.items[start].clone())
    }

    /// Drop played tracks from the front until at most `max` remain, keeping the
    /// current track and everything after it. In shuffle mode the order is
    /// rebuilt, because trimming renumbers the indices it refers to.
    pub fn trim_front_to(&mut self, max: usize) {
        if self.items.len() <= max {
            return;
        }
        // Only items strictly before the current position have been played.
        let droppable = self.current.unwrap_or(0);
        let mut drop = 0;
        while self.items.len() - drop > max && drop < droppable {
            drop += 1;
        }
        if drop == 0 {
            return;
        }
        for _ in 0..drop {
            self.items.pop_front();
        }
        if let Some(c) = self.current {
            self.current = Some(c.saturating_sub(drop));
        }
        if self.shuffle {
            self.rebuild_shuffle();
        }
    }
}
