use std::fmt::Display;

#[derive(Default)]
pub(crate) struct Log {
    pub(crate) buffer: String,
}

impl Log {
    pub(crate) fn println(&mut self, msg: impl Display) {
        println!("{}", msg);
        let msg = msg.to_string();
        self.buffer.push_str(&msg);
        self.buffer.push('\n');
    }
}
