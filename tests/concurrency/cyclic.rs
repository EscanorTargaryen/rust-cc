use rust_cc::{Cc, Context, CopyContext, Finalize, Trace};
use rust_cc::log_pointer::LoggedMutex;

pub struct Cyclic {
    pub(crate) cyclic: LoggedMutex<Option<Cc<Self>>>,
}

impl Finalize for Cyclic {}

unsafe impl Trace for Cyclic {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.cyclic.trace(ctx);
    }

    fn make_copy(&mut self, ctx: &mut CopyContext<'_>) {
        self.cyclic.make_copy(ctx);
    }
}