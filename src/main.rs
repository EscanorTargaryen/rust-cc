use std::thread;
use std::thread::sleep;

use rust_cc::{Cc, collect_cycles, Context, CopyContext, Finalize, THREAD_ACTIONS, Trace};
use rust_cc::log_pointer::LoggedMutex;

struct Cyclic {
    cyclic: LoggedMutex<Option<Cc<Self>>>,
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

fn main() {
    {
        let acyclic = Cc::new(4);

        let s = acyclic.clone();


        let cyclic1 = Cc::new(Cyclic {
            cyclic: LoggedMutex::new(None),
        });
        let cyclic2 = Cc::new(Cyclic {
            cyclic: LoggedMutex::new(None),
        });


        *cyclic1.cyclic.lock().unwrap() = Some(cyclic2.clone());
        *cyclic2.cyclic.lock().unwrap() = Some(cyclic1.clone());

        let s = thread::spawn(|| {
            let _cyclic1 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });
            let _cyclic2 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });

            *_cyclic1.cyclic.lock().unwrap() = Some(_cyclic2.clone());
            *_cyclic2.cyclic.lock().unwrap() = Some(_cyclic1.clone());
        });

        s.join().unwrap();
    }


    { let l = THREAD_ACTIONS.lock().unwrap(); }


    collect_cycles();


    sleep(std::time::Duration::from_secs(20));
}