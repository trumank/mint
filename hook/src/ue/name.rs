use crate::{globals, ue::FString};

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FName {
    pub comparison_index: FNameEntryId,
    pub number: u32,
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
