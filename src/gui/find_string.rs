pub(crate) struct FindString<'data> {
    pub(crate) string: &'data str,
    pub(crate) string_lower: String,
    pub(crate) needle: &'data str,
    pub(crate) needle_lower: String,
    pub(crate) curr: usize,
    pub(crate) curr_match: bool,
    pub(crate) finished: bool,
}

impl<'data> FindString<'data> {
    pub(crate) fn new(string: &'data str, needle: &'data str) -> Self {
        Self {
            string,
            string_lower: string.to_lowercase(),
            needle,
            needle_lower: needle.to_lowercase(),
            curr: 0,
            curr_match: false,
            finished: false,
        }
    }
    pub(crate) fn next_internal(&mut self) -> Option<(bool, &'data str)> {
        if self.finished {
            None
        } else if self.needle.is_empty() {
            self.finished = true;
            Some((false, self.string))
        } else if self.curr_match {
            self.curr_match = false;
            Some((true, &self.string[self.curr - self.needle.len()..self.curr]))
        } else if let Some(index) = self.string_lower[self.curr..].find(&self.needle_lower) {
            let next = self.curr + index;
            let chunk = &self.string[self.curr..next];
            self.curr = next + self.needle.len();
            self.curr_match = true;
            Some((false, chunk))
        } else {
            self.finished = true;
            Some((false, &self.string[self.curr..]))
        }
    }
}

impl<'data> Iterator for FindString<'data> {
    type Item = (bool, &'data str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.string.is_empty() {
            return None;
        }
        // skip empty chunks
        while let Some(chunk) = self.next_internal() {
            if !chunk.1.is_empty() {
                return Some(chunk);
            }
        }
        None
    }
}
