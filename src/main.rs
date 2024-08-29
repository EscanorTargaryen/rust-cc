use std::sync::atomic::Ordering;
use std::thread;
use std::thread::sleep;

use rust_cc::{Cc, collect_cycles, Context, CopyContext, Finalize, Trace};
use rust_cc::collector::{COLLECTOR, init_collector, STOP};
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
    let t3 = thread::spawn(|| {
        let mut i = 0;
        loop {
            let acyclic = Cc::new(4);

            let s = acyclic.clone();
            sleep(std::time::Duration::from_secs(1));
            i += 1;
            if i > 100 { break; };
        }
    });

    let t4 = thread::spawn(|| {
        let mut i = 0;
        loop {
            let _cyclic1 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });
            let _cyclic2 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });

            *_cyclic1.cyclic.lock().unwrap() = Some(_cyclic2.clone());
            *_cyclic2.cyclic.lock().unwrap() = Some(_cyclic1.clone());

            sleep(std::time::Duration::from_secs(2));
            i += 1;
            if i > 100 { break; };
        }
    });

    let t1 = thread::spawn(|| {
        {
            let _cyclic1 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });
            let _cyclic2 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });

            let _cyclic3 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });

            *_cyclic1.cyclic.lock().unwrap() = Some(_cyclic2.clone());
            *_cyclic2.cyclic.lock().unwrap() = Some(_cyclic3.clone());
            *_cyclic3.cyclic.lock().unwrap() = Some(_cyclic1.clone());
        }
    });


    let t2 = thread::spawn(|| {
        {
            let _cyclic1 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });
            let _cyclic2 = Cc::new(Cyclic {
                cyclic: LoggedMutex::new(None),
            });

            *_cyclic1.cyclic.lock().unwrap() = Some(_cyclic2.clone());
            *_cyclic2.cyclic.lock().unwrap() = Some(_cyclic1.clone());
        }
    });
    sleep(std::time::Duration::from_secs(2));
    collect_cycles();
    init_collector();
    let _ = t1.join();
    let _ = t2.join();
    let _ = t3.join();
    let _ = t4.join();
    let o = COLLECTOR.get().unwrap().lock().unwrap().take();
    if let Some(o) = o {
        STOP.store(true, Ordering::Release);

        collect_cycles();

        let _ = o.join();
    }
}