use std::fmt::Debug;

use super::TArray;

pub trait UEHash {
    fn ue_hash(&self) -> u32;
}

#[derive(Default, Debug)]
#[repr(C)]
struct TSetElement<V> {
    value: V,
    hash_next_id: FSetElementId,
    hash_index: i32,
}
impl<K: UEHash, V> UEHash for TSetElement<TTuple<K, V>> {
    fn ue_hash(&self) -> u32 {
        self.value.a.ue_hash()
    }
}

#[derive(Default, Clone, Copy)]
#[repr(C)]
pub struct FSetElementId {
    index: i32,
}
impl FSetElementId {
    pub fn is_valid(self) -> bool {
        self.index != -1
    }
}
impl Debug for FSetElementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FSetElementId({:?})", self.index)
    }
}

#[derive(Default, Debug)]
#[repr(C)]
struct TTuple<A, B> {
    a: A,
    b: B,
}

#[repr(C)]
union TSparseArrayElementOrFreeListLink<E> {
    element: std::mem::ManuallyDrop<E>,
    list_link: ListLink,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct ListLink {
    next_free_index: i32,
    prev_free_index: i32,
}

#[derive(Debug)]
#[repr(C)]
struct TInlineAllocator<const N: usize, V> {
    inline_data: [V; N],
    secondary_data: *const V, // TSizedHeapAllocator<32>::ForElementType<unsigned int>,
}
impl<const N: usize, V: Default + Copy> Default for TInlineAllocator<N, V> {
    fn default() -> Self {
        Self {
            inline_data: [Default::default(); N],
            secondary_data: std::ptr::null(),
        }
    }
}

impl<const N: usize, V> TInlineAllocator<N, V> {
    fn get_allocation(&self) -> *const V {
        if !self.secondary_data.is_null() {
            self.secondary_data
        } else {
            self.inline_data.as_ptr()
        }
    }
}

#[derive(Default, Debug)]
#[repr(C)]
struct TBitArray {
    allocator_instance: TInlineAllocator<4, u32>,
    num_bits: i32,
    max_bits: i32,
}
impl TBitArray {
    fn get_data(&self) -> *const u32 {
        self.allocator_instance.get_allocation()
    }

    fn index(&self, index: usize) -> FBitReference<'_> {
        assert!(index < self.num_bits as usize);
        let num_bits_per_dword = 32;
        FBitReference {
            data: unsafe { &*self.get_data().add(index / num_bits_per_dword) },
            mask: 1 << (index & (num_bits_per_dword - 1)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct FBitReference<'data> {
    data: &'data u32,
    mask: u32,
}
impl FBitReference<'_> {
    fn bool(self) -> bool {
        (self.data & self.mask) != 0
    }
}

#[repr(C)]
struct TSparseArray<E> {
    data: TArray<TSparseArrayElementOrFreeListLink<E>>,
    allocation_flags: TBitArray,
    first_free_index: i32,
    num_free_indices: i32,
}
impl<E> Default for TSparseArray<E> {
    fn default() -> Self {
        Self {
            data: Default::default(),
            allocation_flags: Default::default(),
            first_free_index: 0,
            num_free_indices: 0,
        }
    }
}
impl<E> TSparseArray<E> {
    fn index(&self, index: usize) -> &E {
        assert!(index < self.data.len() && index < self.allocation_flags.num_bits as usize);
        assert!(self.allocation_flags.index(index).bool());
        unsafe { &self.data.as_slice()[index].element }
    }
}

struct DbgTSparseArrayData<'a, E>(&'a TSparseArray<E>);
impl<E: Debug> Debug for DbgTSparseArrayData<'_, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_list();
        for i in 0..(self.0.allocation_flags.num_bits as usize) {
            if self.0.allocation_flags.index(i).bool() {
                dbg.entry(self.0.index(i));
            }
        }
        dbg.finish()
    }
}

impl<E: Debug> Debug for TSparseArray<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TSparseArray")
            .field("data", &DbgTSparseArrayData(self))
            .field("allocation_flags", &self.allocation_flags)
            .field("first_free_index", &self.first_free_index)
            .field("num_free_indices", &self.num_free_indices)
            .finish()
    }
}

#[repr(C)]
pub struct TMap<K: UEHash, V> {
    elements: TSparseArray<TSetElement<TTuple<K, V>>>,
    hash: TInlineAllocator<1, FSetElementId>,
    hash_size: i32,
}
impl<K: UEHash, V> TMap<K, V> {
    fn hash(&self) -> &[FSetElementId] {
        unsafe { std::slice::from_raw_parts(self.hash.get_allocation(), self.hash_size as usize) }
    }
}
impl<K: UEHash, V> Default for TMap<K, V> {
    fn default() -> Self {
        Self {
            elements: Default::default(),
            hash: Default::default(),
            hash_size: 0,
        }
    }
}
impl<K: UEHash + Debug, V: Debug> Debug for TMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TMap")
            .field("elements", &self.elements)
            .field("hash", &self.hash())
            .finish()
    }
}

impl<K: PartialEq + UEHash, V> TMap<K, V> {
    pub fn find(&self, key: K) -> Option<&V> {
        let id = self.find_id(key);
        if id.is_valid() {
            Some(&self.elements.index(id.index as usize).value.b)
        } else {
            None
        }
    }
    pub fn find_id(&self, key: K) -> FSetElementId {
        if self.elements.data.len() != self.elements.num_free_indices as usize {
            let key_hash = key.ue_hash();
            let hash = &self.hash();

            let mut i: FSetElementId =
                hash[(((self.hash_size as i64) - 1) & (key_hash as i64)) as usize];

            if i.is_valid() {
                loop {
                    let elm = self.elements.index(i.index as usize);

                    if elm.value.a == key {
                        return i;
                    }

                    i = elm.hash_next_id;
                    if !i.is_valid() {
                        break;
                    }
                }
            }
        }

        FSetElementId { index: -1 }
    }
}

#[cfg(test)]
mod test {
    use crate::ue::FName;

    use super::*;
    const _: [u8; 0x50] = [0; std::mem::size_of::<TMap<FName, [u8; 0x20]>>()];
    const _: [u8; 0x38] =
        [0; std::mem::size_of::<TSparseArray<TSetElement<TTuple<FName, [u8; 0x20]>>>>()];
    const _: [u8; 0x10] = [0; std::mem::size_of::<TInlineAllocator<1, FSetElementId>>()];
}
