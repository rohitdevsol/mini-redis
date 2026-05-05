const K_RESIZING_WORK: usize = 128; // nodes to migrate per operation
const K_MAX_LOAD_FACTOR: usize = 8; // resize when size/buckets >= this

pub struct HNode {
    pub hcode: u64,
}

pub struct Entry {
    pub node: HNode,
    pub key: String,
    pub val: String,
}

impl Entry {
    pub fn new(key: String, val: String) -> Self {
        let hcode = str_hash(&key);
        Entry {
            node: HNode { hcode },
            key,
            val,
        }
    }
}
pub struct ChainNode {
    pub entry: Entry,
    pub next: Option<Box<ChainNode>>,
}

// fix size hashtable .. array of chains

pub struct HTab {
    pub buckets: Vec<Option<Box<ChainNode>>>,
    pub mask: usize, // always size-1, for fast indexing ( 2 ki power vala thingy using & )
    pub size: usize, // how many entries currently in here
}

impl HTab {
    pub fn new(n: usize) -> Self {
        assert!(n > 0 && n.is_power_of_two());

        HTab {
            buckets: (0..n).map(|_| None).collect(),
            mask: n - 1,
            size: 0,
        }
    }
    fn idx(&self, hcode: u64) -> usize {
        (hcode as usize) & self.mask
    }

    pub fn insert(&mut self, entry: Entry) {
        let idx = self.idx(entry.node.hcode);

        // take ownership of whatever is currently at this bucket
        // put new node at the front of the chain
        // point new node's next at the old chain
        let old_chain = self.buckets[idx].take();
        self.buckets[idx] = Some(
            Box::new(ChainNode {
                next: old_chain,
                entry,
            })
        );
        self.size += 1;
    }

    // look up a key in this table
    // returns a reference to the Entry if found
    pub fn lookup(&self, key: &str, hcode: u64) -> Option<&Entry> {
        if self.buckets.is_empty() {
            return None;
        }
        let idx = self.idx(hcode);
        let mut current = self.buckets[idx].as_ref();

        // walk the chain
        while let Some(node) = current {
            if node.entry.node.hcode == hcode && node.entry.key == key {
                return Some(&node.entry);
            }
            current = node.next.as_ref();
        }
        None
    }

    // look up a key and return mutable reference
    pub fn lookup_mut(&mut self, key: &str, hcode: u64) -> Option<&mut Entry> {
        if self.buckets.is_empty() {
            return None;
        }
        let idx = self.idx(hcode);
        let mut current = self.buckets[idx].as_mut();

        while let Some(node) = current {
            if node.entry.node.hcode == hcode && node.entry.key == key {
                return Some(&mut node.entry);
            }
            current = node.next.as_mut();
        }
        None
    }

    // remove a key from this table
    // returns the Entry if found
    // mirrors h_detach from the book
    pub fn remove(&mut self, key: &str, hcode: u64) -> Option<Entry> {
        if self.buckets.is_empty() {
            return None;
        }
        let idx = self.idx(hcode);

        // we need to rewire the chain to skip the removed node
        // this is the **from pointer trick from the book
        // done safely in Rust by rebuilding the chain
        let mut current = self.buckets[idx].take();
        let mut result = None;
        let mut rebuilt: Option<Box<ChainNode>> = None;
        let mut tail: *mut Option<Box<ChainNode>> = &mut rebuilt;

        while let Some(mut node) = current {
            if node.entry.node.hcode == hcode && node.entry.key == key {
                // found it — skip this node, stitch chain back together
                current = node.next.take();
                result = Some(node.entry);
                // attach remaining chain to the rebuilt chain
                unsafe {
                    *tail = current;
                }
                break;
            } else {
                current = node.next.take();
                unsafe {
                    *tail = Some(node);
                    tail = &mut (*tail).as_mut().unwrap().next;
                }
            }
        }

        self.buckets[idx] = rebuilt;
        if result.is_some() {
            self.size -= 1;
        }
        result
    }

    // pop one entry from this table for migration
    // used during resizing — grabs whatever is at resizing_pos
    pub fn pop_one(&mut self, pos: &mut usize) -> Option<Entry> {
        while *pos <= self.mask {
            if self.buckets[*pos].is_some() {
                let key = self.buckets[*pos].as_ref().unwrap().entry.key.clone();
                let hcode = self.buckets[*pos].as_ref().unwrap().entry.node.hcode;
                return self.remove(&key, hcode);
            }
            *pos += 1;
        }
        None
    }
}

pub fn str_hash(key: &str) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for byte in key.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

pub struct HMap {
    ht1: HTab, // active table — all new inserts go here
    ht2: HTab, // old table — being drained into ht1
    resizing_pos: usize, // which bucket in ht2 we are currently draining
}

impl HMap {
    pub fn new() -> Self {
        HMap {
            ht1: HTab::new(4), // start small, 4 buckets
            ht2: HTab { buckets: vec![], mask: 0, size: 0 },
            resizing_pos: 0,
        }
    }

    // mirrors hm_help_resizing
    // called on EVERY operation — moves a few nodes from ht2 to ht1
    fn help_resizing(&mut self) {
        if self.ht2.buckets.is_empty() {
            return; // not resizing right now
        }

        let mut work = 0;
        while work < K_RESIZING_WORK && self.ht2.size > 0 {
            match self.ht2.pop_one(&mut self.resizing_pos) {
                Some(entry) => {
                    self.ht1.insert(entry); // move to ht1
                    work += 1;
                }
                None => {
                    break;
                } // ht2 fully drained
            }
        }

        // if ht2 is empty resizing is done
        if self.ht2.size == 0 {
            self.ht2 = HTab { buckets: vec![], mask: 0, size: 0 };
            self.resizing_pos = 0;
        }
    }

    // mirrors hm_start_resizing
    fn start_resizing(&mut self) {
        // swap ht1 into ht2
        // make a fresh ht1 twice the size
        let new_size = (self.ht1.mask + 1) * 2;
        let old_ht1 = std::mem::replace(&mut self.ht1, HTab::new(new_size));
        self.ht2 = old_ht1;
        self.resizing_pos = 0;
    }

    // mirrors hm_lookup
    pub fn get(&mut self, key: &str) -> Option<String> {
        self.help_resizing();
        let hcode = str_hash(key);

        if let Some(e) = self.ht1.lookup(key, hcode) {
            return Some(e.val.clone());
        }
        self.ht2.lookup(key, hcode).map(|e| e.val.clone())
    }
    // mirrors hm_insert
    pub fn set(&mut self, key: String, val: String) {
        self.help_resizing();
        let hcode = str_hash(&key);

        // if key already exists update it in place
        if let Some(entry) = self.ht1.lookup_mut(&key, hcode) {
            entry.val = val;
            return;
        }
        if let Some(entry) = self.ht2.lookup_mut(&key, hcode) {
            entry.val = val;
            return;
        }

        // new key — insert into ht1
        self.ht1.insert(Entry::new(key, val));

        // check if we need to start resizing
        let load = self.ht1.size / (self.ht1.mask + 1);
        if load >= K_MAX_LOAD_FACTOR && self.ht2.buckets.is_empty() {
            self.start_resizing();
        }
    }

    // mirrors hm_pop
    pub fn del(&mut self, key: &str) -> bool {
        self.help_resizing();
        let hcode = str_hash(key);

        // try ht1 first then ht2
        if self.ht1.remove(key, hcode).is_some() {
            return true;
        }
        self.ht2.remove(key, hcode).is_some()
    }

    pub fn len(&self) -> usize {
        self.ht1.size + self.ht2.size
    }
}
