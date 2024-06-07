use crate::{globals, ue::FString};

use super::UEHash;

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum EFindName {
    Find,
    Add,
    ReplaceNotSafeForThreading,
}

#[derive(Default, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
#[repr(C)]
pub struct FName {
    pub comparison_index: FNameEntryId,
    pub number: u32,
}
impl FName {
    pub fn new(string: &FString) -> FName {
        let mut ret = FName::default();
        unsafe { globals().fname_ctor_wchar()(&mut ret, string.as_ptr(), EFindName::Add) };
        ret
    }
    pub fn find(string: &FString) -> FName {
        let mut ret = FName::default();
        unsafe { globals().fname_ctor_wchar()(&mut ret, string.as_ptr(), EFindName::Find) };
        ret
    }
}
impl std::fmt::Debug for FName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FName({self})")
    }
}

#[derive(Default, Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
#[repr(C)]
pub struct FNameEntryId {
    pub value: u32,
}

impl std::fmt::Display for FName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut string = FString::new();
        unsafe {
            (globals().fname_to_string())(self, &mut string);
        };
        write!(f, "{string}")
    }
}
impl UEHash for FNameEntryId {
    fn ue_hash(&self) -> u32 {
        let value = self.value;
        (value >> 4) + value.wrapping_mul(0x10001) + (value >> 0x10).wrapping_mul(0x80001)
    }
}

impl UEHash for FName {
    fn ue_hash(&self) -> u32 {
        self.comparison_index.ue_hash() + self.number
    }
}
