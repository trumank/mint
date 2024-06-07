use crate::{globals, ue::FString};

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum EFindName {
    Find,
    Add,
    ReplaceNotSafeForThreading,
}

#[derive(Default, Clone, Copy)]
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

#[derive(Default, Debug, Clone, Copy)]
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
