pub(crate) struct Gated<T> {
    value: Option<T>,
    hold: bool,
}

impl<T> Gated<T> {
    pub(crate) fn empty() -> Self {
        Self {
            value: None,
            hold: false,
        }
    }
    pub(crate) fn held(value: T) -> Self {
        Self {
            value: Some(value),
            hold: true,
        }
    }
    pub(crate) fn ready(value: T) -> Self {
        Self {
            value: Some(value),
            hold: false,
        }
    }

    pub(crate) fn release(&mut self) {
        self.hold = false;
    }

    pub(crate) fn get(&self) -> Option<&T> {
        if self.hold { None } else { self.value.as_ref() }
    }

    pub(crate) fn take(&mut self) -> Option<T> {
        if self.hold { None } else { self.value.take() }
    }
}
