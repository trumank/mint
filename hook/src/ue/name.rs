use crate::{globals, ue::FString};

use super::EFindName;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FName {
    pub comparison_index: FNameEntryId,
    pub number: u32,
}

impl FName {
    fn ctor(string: &str, find_type: EFindName) -> Self {
        let wstr = string.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut new = Self {
            comparison_index: FNameEntryId { value: 0 },
            number: 0,
        };
        unsafe {
            (globals().fname_ctor())(&mut new, wstr.as_ptr(), find_type);
        }
        new
    }
    pub fn new(string: &str) -> Self {
        Self::ctor(string, EFindName::Add)
    }
    pub fn find(string: &str) -> Option<Self> {
        let new = Self::ctor(string, EFindName::Find);
        (new.comparison_index.value != 0 || new.number != 0).then_some(new)
    }
}

#[derive(Debug, Clone, Copy)]
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
