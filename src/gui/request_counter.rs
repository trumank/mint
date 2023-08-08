/// Simple counter that returns a new ID each time it is called
#[derive(Default)]
pub struct RequestCounter(u32);

impl RequestCounter {
    /// Get next ID
    pub fn next(&mut self) -> RequestID {
        let id = self.0;
        self.0 += 1;
        RequestID { id }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RequestID {
    id: u32,
}
