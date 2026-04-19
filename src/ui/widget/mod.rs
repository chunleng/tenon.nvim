use dyn_clone::{DynClone, clone_trait_object};
use nvim_oxi::Result as OxiResult;

pub mod display;

pub trait Widget: DynClone + Send + Sync {
    fn render(&mut self) -> OxiResult<()>;
}

// Use for transition to the new UI modules
#[derive(Clone)]
pub struct NoWidget;
impl Widget for NoWidget {
    fn render(&mut self) -> OxiResult<()> {
        Ok(())
    }
}

clone_trait_object!(Widget);
