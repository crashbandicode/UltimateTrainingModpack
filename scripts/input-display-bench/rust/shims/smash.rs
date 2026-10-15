//! Host shim for `smash::ui2d::SmashTextBox`.
//! Text is copied into a fixed buffer so the shim itself does not allocate.

pub mod ui2d {
    use skyline::nn::ui2d::TextBox;

    pub trait SmashTextBox {
        unsafe fn set_text_string(&mut self, text: &str);
    }

    impl SmashTextBox for TextBox {
        unsafe fn set_text_string(&mut self, text: &str) {
            self.write_text(text);
        }
    }
}
