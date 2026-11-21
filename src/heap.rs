use crate::CollectionKind;
use crate::value::Value;

pub const INITIAL_HEAP_BYTES: usize = 1024 * 1024;
const COLLECTION_INTERVAL: usize = 256;
const VALUE_BYTES: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObjectRef(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapStats {
    pub capacity_bytes: usize,
    pub used_bytes: usize,
    pub live_allocations: usize,
    pub collections: usize,
}

struct Entry {
    offset: usize,
    length: usize,
    kind: ObjectKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjectKind {
    String,
    Collection(CollectionKind),
    /// A record instance of the type with this index in the type table.
    Record(usize),
}

pub(crate) struct Heap {
    storage: Vec<u8>,
    entries: Vec<Option<Entry>>,
    free_slots: Vec<usize>,
    used: usize,
    allocations_since_gc: usize,
    gc_interval: usize,
    collections: usize,
}

impl Heap {
    pub(crate) fn new() -> Result<Self, String> {
        let mut storage = Vec::new();
        storage
            .try_reserve_exact(INITIAL_HEAP_BYTES)
            .map_err(|e| e.to_string())?;
        storage.resize(INITIAL_HEAP_BYTES, 0);
        Ok(Self {
            storage,
            entries: Vec::new(),
            free_slots: Vec::new(),
            used: 0,
            allocations_since_gc: 0,
            gc_interval: COLLECTION_INTERVAL,
            collections: 0,
        })
    }

    pub(crate) fn allocate(
        &mut self,
        text: &str,
        roots: &[ObjectRef],
    ) -> Result<ObjectRef, String> {
        self.allocate_bytes(text.as_bytes(), ObjectKind::String, roots, &[])
    }

    pub(crate) fn new_collection(
        &mut self,
        kind: CollectionKind,
        values: &[Value],
        roots: &[ObjectRef],
    ) -> Result<ObjectRef, String> {
        let bytes = encode(values)?;
        self.allocate_bytes(&bytes, ObjectKind::Collection(kind), roots, values)
    }

    /// Allocates a record of type `type_id` whose fields are `values`.
    pub(crate) fn new_record(
        &mut self,
        type_id: usize,
        values: &[Value],
        roots: &[ObjectRef],
    ) -> Result<ObjectRef, String> {
        let bytes = encode(values)?;
        self.allocate_bytes(&bytes, ObjectKind::Record(type_id), roots, values)
    }

    /// The type-table index of a record.
    pub(crate) fn record_type(&self, reference: ObjectRef) -> Result<usize, String> {
        match self.entry(reference)?.kind {
            ObjectKind::Record(type_id) => Ok(type_id),
            _ => Err("managed object is not a record".into()),
        }
    }

    pub(crate) fn record_fields(&self, reference: ObjectRef) -> Result<Vec<Value>, String> {
        let entry = self.entry(reference)?;
        if !matches!(entry.kind, ObjectKind::Record(_)) {
            return Err("managed object is not a record".into());
        }
        decode(&self.storage[entry.offset..entry.offset + entry.length])
    }

    /// Reads one field without decoding the whole payload.
    pub(crate) fn record_field(&self, reference: ObjectRef, index: usize) -> Result<Value, String> {
        let range = self.field_range(reference, index)?;
        decode_value(&self.storage[range])
    }

    /// Overwrites one field in place. A record never changes size, so this
    /// neither allocates nor moves anything.
    pub(crate) fn set_record_field(
        &mut self,
        reference: ObjectRef,
        index: usize,
        value: Value,
    ) -> Result<(), String> {
        let range = self.field_range(reference, index)?;
        let mut bytes = Vec::with_capacity(VALUE_BYTES);
        encode_value(value, &mut bytes)?;
        self.storage[range].copy_from_slice(&bytes);
        Ok(())
    }

    fn field_range(
        &self,
        reference: ObjectRef,
        index: usize,
    ) -> Result<std::ops::Range<usize>, String> {
        let entry = self.entry(reference)?;
        if !matches!(entry.kind, ObjectKind::Record(_)) {
            return Err("managed object is not a record".into());
        }
        let start = index
            .checked_mul(VALUE_BYTES)
            .filter(|start| start + VALUE_BYTES <= entry.length)
            .ok_or("record field index out of range")?;
        Ok(entry.offset + start..entry.offset + start + VALUE_BYTES)
    }

    /// `extra` holds values (the new payload) that must survive a collection
    /// triggered here; the combined root list is only built when collecting.
    fn prepare(&mut self, size: usize, roots: &[ObjectRef], extra: &[Value]) -> Result<(), String> {
        let required = self.used.checked_add(size).ok_or("heap size overflow")?;
        if required > self.storage.len() || self.allocations_since_gc >= self.gc_interval {
            self.collect(&payload_roots(extra, roots))?;
        }
        let required = self.used.checked_add(size).ok_or("heap size overflow")?;
        if required > self.storage.len() {
            let capacity = required
                .checked_next_power_of_two()
                .ok_or("heap size overflow")?;
            self.storage
                .try_reserve_exact(capacity - self.storage.len())
                .map_err(|e| e.to_string())?;
            self.storage.resize(capacity, 0);
        }
        Ok(())
    }

    fn allocate_bytes(
        &mut self,
        bytes: &[u8],
        kind: ObjectKind,
        roots: &[ObjectRef],
        extra: &[Value],
    ) -> Result<ObjectRef, String> {
        let size = bytes.len().max(1);
        self.prepare(size, roots, extra)?;
        let slot = if let Some(slot) = self.free_slots.pop() {
            slot
        } else {
            self.entries.try_reserve(1).map_err(|e| e.to_string())?;
            self.entries.push(None);
            self.entries.len() - 1
        };
        self.storage[self.used..self.used + bytes.len()].copy_from_slice(bytes);
        self.entries[slot] = Some(Entry {
            offset: self.used,
            length: bytes.len(),
            kind,
        });
        self.used += size;
        self.allocations_since_gc += 1;
        Ok(ObjectRef(slot))
    }

    fn entry(&self, reference: ObjectRef) -> Result<&Entry, String> {
        self.entries
            .get(reference.0)
            .and_then(Option::as_ref)
            .ok_or_else(|| "invalid managed object reference".into())
    }

    pub(crate) fn get(&self, reference: ObjectRef) -> Result<&str, String> {
        let entry = self.entry(reference)?;
        if entry.kind != ObjectKind::String {
            return Err("managed object is not a string".into());
        }
        std::str::from_utf8(&self.storage[entry.offset..entry.offset + entry.length])
            .map_err(|e| format!("invalid managed string: {e}"))
    }

    pub(crate) fn collection(
        &self,
        reference: ObjectRef,
    ) -> Result<(CollectionKind, Vec<Value>), String> {
        let entry = self.entry(reference)?;
        let ObjectKind::Collection(kind) = entry.kind else {
            return Err("managed object is not a collection".into());
        };
        let values = decode(&self.storage[entry.offset..entry.offset + entry.length])?;
        Ok((kind, values))
    }

    /// The kind of a collection without decoding its payload.
    pub(crate) fn collection_kind(&self, reference: ObjectRef) -> Result<CollectionKind, String> {
        match self.entry(reference)?.kind {
            ObjectKind::Collection(kind) => Ok(kind),
            ObjectKind::String | ObjectKind::Record(_) => {
                Err("managed object is not a collection".into())
            }
        }
    }

    pub(crate) fn replace_collection(
        &mut self,
        reference: ObjectRef,
        values: &[Value],
        roots: &[ObjectRef],
    ) -> Result<(), String> {
        let kind = self.entry(reference)?.kind;
        if !matches!(kind, ObjectKind::Collection(_)) {
            return Err("managed object is not a collection".into());
        }
        let bytes = encode(values)?;
        let mut roots = roots.to_vec();
        roots.push(reference);
        self.prepare(bytes.len().max(1), &roots, values)?;
        self.storage[self.used..self.used + bytes.len()].copy_from_slice(&bytes);
        self.entries[reference.0] = Some(Entry {
            offset: self.used,
            length: bytes.len(),
            kind,
        });
        self.used += bytes.len().max(1);
        self.allocations_since_gc += 1;
        Ok(())
    }

    pub(crate) fn collect(&mut self, roots: &[ObjectRef]) -> Result<(), String> {
        let mut marked = vec![false; self.entries.len()];
        let mut pending = roots.to_vec();
        while let Some(root) = pending.pop() {
            let entry = self.entry(root)?;
            if marked[root.0] {
                continue;
            }
            marked[root.0] = true;
            if matches!(
                entry.kind,
                ObjectKind::Collection(_) | ObjectKind::Record(_)
            ) {
                let values = decode(&self.storage[entry.offset..entry.offset + entry.length])?;
                pending.extend(values.into_iter().filter_map(Value::reference));
            }
        }
        let mut live = Vec::new();
        self.free_slots.clear();
        for (index, entry) in self.entries.iter_mut().enumerate() {
            if marked[index] {
                live.push(index);
            } else {
                *entry = None;
                self.free_slots.push(index);
            }
        }
        // Reused handle slots are not necessarily in allocation order.
        live.sort_unstable_by_key(|index| self.entries[*index].as_ref().unwrap().offset);
        let mut used = 0;
        for index in live {
            let entry = self.entries[index].as_mut().unwrap();
            self.storage
                .copy_within(entry.offset..entry.offset + entry.length, used);
            entry.offset = used;
            used += entry.length.max(1);
        }
        self.used = used;
        let target = used
            .checked_mul(2)
            .and_then(usize::checked_next_power_of_two)
            .ok_or("heap size overflow")?
            .max(INITIAL_HEAP_BYTES);
        if target < self.storage.len() {
            self.storage.truncate(target);
            self.storage.shrink_to_fit();
        }
        self.allocations_since_gc = 0;
        // Grow the interval with the live set so building a large structure
        // does not re-mark it every few allocations (amortised linear).
        self.gc_interval = COLLECTION_INTERVAL.max(self.entries.len() - self.free_slots.len());
        self.collections += 1;
        Ok(())
    }

    pub(crate) fn stats(&self) -> HeapStats {
        HeapStats {
            capacity_bytes: self.storage.len(),
            used_bytes: self.used,
            live_allocations: self.entries.len() - self.free_slots.len(),
            collections: self.collections,
        }
    }
}

fn payload_roots(values: &[Value], roots: &[ObjectRef]) -> Vec<ObjectRef> {
    roots
        .iter()
        .copied()
        .chain(values.iter().filter_map(|value| value.reference()))
        .collect()
}

fn encode(values: &[Value]) -> Result<Vec<u8>, String> {
    let size = values
        .len()
        .checked_mul(VALUE_BYTES)
        .ok_or("collection size overflow")?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|e| e.to_string())?;
    for value in values {
        encode_value(*value, &mut bytes)?;
    }
    Ok(bytes)
}

/// Appends the `VALUE_BYTES`-byte encoding of one value.
fn encode_value(value: Value, bytes: &mut Vec<u8>) -> Result<(), String> {
    let (tag, payload) = match value {
        Value::Number(n) => (0, n.to_le_bytes()),
        Value::Int(n) => (3, n.to_le_bytes()),
        Value::Nothing => (5, [0; 8]),
        Value::String(reference) => (1, reference_bytes(reference)?),
        Value::Collection(reference) => (2, reference_bytes(reference)?),
        Value::Record(reference) => (4, reference_bytes(reference)?),
    };
    bytes.push(tag);
    bytes.extend_from_slice(&payload);
    Ok(())
}

fn reference_bytes(reference: ObjectRef) -> Result<[u8; 8], String> {
    let index = u64::try_from(reference.0).map_err(|_| "managed reference overflow")?;
    Ok(index.to_le_bytes())
}

fn decode_value(chunk: &[u8]) -> Result<Value, String> {
    let payload: [u8; 8] = chunk
        .get(1..VALUE_BYTES)
        .and_then(|payload| payload.try_into().ok())
        .ok_or("invalid collection payload")?;
    let reference = || {
        usize::try_from(u64::from_le_bytes(payload))
            .map(ObjectRef)
            .map_err(|_| "managed reference overflow".to_string())
    };
    match chunk[0] {
        0 => Ok(Value::Number(f64::from_le_bytes(payload))),
        3 => Ok(Value::Int(i64::from_le_bytes(payload))),
        1 => Ok(Value::String(reference()?)),
        2 => Ok(Value::Collection(reference()?)),
        4 => Ok(Value::Record(reference()?)),
        5 => Ok(Value::Nothing),
        _ => Err("invalid collection value tag".into()),
    }
}

fn decode(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let (chunks, remainder) = bytes.as_chunks::<VALUE_BYTES>();
    if !remainder.is_empty() {
        return Err("invalid collection payload length".into());
    }
    chunks.iter().map(|chunk| decode_value(chunk)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_preserves_roots_and_shrinks_to_initial_capacity() {
        let mut heap = Heap::new().unwrap();
        assert_eq!(heap.stats().capacity_bytes, INITIAL_HEAP_BYTES);
        let small = heap.allocate("keep", &[]).unwrap();
        let large_text = "x".repeat(INITIAL_HEAP_BYTES + 1);
        let large = heap.allocate(&large_text, &[small]).unwrap();
        assert!(heap.stats().capacity_bytes > INITIAL_HEAP_BYTES);
        heap.collect(&[small, large]).unwrap();
        assert_eq!(heap.get(large).unwrap(), large_text);
        assert_eq!(heap.get(small).unwrap(), "keep");
        heap.collect(&[small]).unwrap();
        assert_eq!(heap.stats().capacity_bytes, INITIAL_HEAP_BYTES);
        assert_eq!(heap.stats().used_bytes, 4);
        assert_eq!(heap.stats().live_allocations, 1);
    }

    #[test]
    fn compaction_preserves_reused_slots_and_empty_unicode_strings() {
        let mut heap = Heap::new().unwrap();
        heap.allocate("discard", &[]).unwrap();
        let first = heap.allocate("first", &[]).unwrap();
        heap.collect(&[first]).unwrap();
        let second = heap.allocate("second", &[first]).unwrap();
        let unicode = heap.allocate("\u{1f600}", &[first, second]).unwrap();
        let empty = heap.allocate("", &[first, second, unicode]).unwrap();
        heap.collect(&[first, second, unicode, empty]).unwrap();
        assert_eq!(heap.get(first).unwrap(), "first");
        assert_eq!(heap.get(second).unwrap(), "second");
        assert_eq!(heap.get(unicode).unwrap(), "\u{1f600}");
        assert_eq!(heap.get(empty).unwrap(), "");
    }

    #[test]
    fn periodic_collection_reclaims_garbage_and_reuses_metadata() {
        let mut heap = Heap::new().unwrap();
        let root = heap.allocate("root", &[]).unwrap();
        for _ in 0..2000 {
            heap.allocate("temporary", &[root]).unwrap();
        }
        assert!(heap.stats().collections > 0);
        assert!(heap.entries.len() <= COLLECTION_INTERVAL + 1);
        heap.collect(&[root]).unwrap();
        assert_eq!(heap.stats().used_bytes, 4);
        assert_eq!(heap.get(root).unwrap(), "root");
    }

    #[test]
    fn collection_payloads_use_the_arena_and_resize_it() {
        let mut heap = Heap::new().unwrap();
        let values = vec![Value::Number(42.0); INITIAL_HEAP_BYTES / VALUE_BYTES + 1];
        let array = heap
            .new_collection(CollectionKind::Array, &values, &[])
            .unwrap();
        assert_eq!(heap.stats().used_bytes, values.len() * VALUE_BYTES);
        assert!(heap.stats().capacity_bytes > INITIAL_HEAP_BYTES);
        heap.collect(&[array]).unwrap();
        let (_, decoded) = heap.collection(array).unwrap();
        assert_eq!(decoded.len(), values.len());
        assert!(matches!(decoded.last(), Some(Value::Number(42.0))));
        heap.collect(&[]).unwrap();
        assert_eq!(heap.stats().capacity_bytes, INITIAL_HEAP_BYTES);
        assert_eq!(heap.stats().used_bytes, 0);
    }
}
