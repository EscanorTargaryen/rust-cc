use std::sync::Mutex;
use std::thread;
use std::thread::sleep;

use rust_cc::{Cc, collect_cycles, Context, Finalize, THREAD_ACTIONS, Trace};

struct Cyclic {
    cyclic: Mutex<Option<Cc<Self>>>,
}

impl Finalize for Cyclic {}

unsafe impl Trace for Cyclic {
    fn trace(&self, _: &mut Context<'_>) {}
}

fn main() {
    {
        let acyclic = Cc::new(4);

        let s = acyclic.clone();


        let cyclic1 = Cc::new(Cyclic {
            cyclic: Mutex::new(None),
        });
        let cyclic2 = Cc::new(Cyclic {
            cyclic: Mutex::new(None),
        });


        *cyclic1.cyclic.lock().unwrap() = Some(cyclic2.clone());
        *cyclic2.cyclic.lock().unwrap() = Some(cyclic1.clone());

        let s = thread::spawn(|| {
            let _cyclic1 = Cc::new(Cyclic {
                cyclic: Mutex::new(None),
            });
            let _cyclic2 = Cc::new(Cyclic {
                cyclic: Mutex::new(None),
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