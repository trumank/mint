use super::TArray;

pub type FString = TArray<u16>;

impl From<&str> for FString {
    fn from(value: &str) -> Self {
        Self::from(
            widestring::U16CString::from_str(value)
                .unwrap()
                .as_slice_with_nul(),
        )
    }
}

impl std::fmt::Display for FString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let slice = self.as_slice();
        let last = slice.len()
            - slice
                .iter()
                .cloned()
                .rev()
                .position(|c| c != 0)
                .unwrap_or_default();
        write!(
            f,
            "{}",
            widestring::U16Str::from_slice(&slice[..last])
                .to_string()
                .unwrap()
        )
    }
}
