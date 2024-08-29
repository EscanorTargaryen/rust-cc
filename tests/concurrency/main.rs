use std::sync::atomic::Ordering;
use std::thread;
use std::thread::sleep;

use rust_cc::{Cc, Context, CopyContext, Finalize, Trace};
use rust_cc::collector::{collect_and_stop, N_ACYCLIC_DROPPED, N_CYCLIC_DROPPED};
use rust_cc::log_pointer::LoggedMutex;

pub struct Cyclic {
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


#[test]
fn collect_acyclic() {
    let t = thread::spawn(|| {
        let _ = Cc::new(4);
    });

    let _ = Cc::new(4);
    sleep(std::time::Duration::from_secs(2));

    let _ = t.join();

    collect_and_stop();

    assert_eq!(N_ACYCLIC_DROPPED.load(Ordering::Relaxed), 2);
    assert_eq!(N_CYCLIC_DROPPED.load(Ordering::Relaxed), 0);
}

#[test]
fn collect_cyclic() {
    let t = thread::spawn(|| {
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

    t.join().unwrap();

    collect_and_stop();

    assert_eq!(N_ACYCLIC_DROPPED.load(Ordering::Relaxed), 0);
    assert_eq!(N_CYCLIC_DROPPED.load(Ordering::Relaxed), 6);
}


