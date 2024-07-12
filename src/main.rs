use std::sync::Mutex;
use std::thread;
use std::thread::sleep;

use rust_cc::{Cc, collect_cycles, COLLECTOR, Context, Finalize, Trace};

struct Cyclic {
    cyclic: Mutex<Option<Cc<Self>>>,
}

impl Finalize for Cyclic {}

unsafe impl Trace for Cyclic {
    fn trace(&self, _: &mut Context<'_>) {}
}

fn main() {
    {
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

 

    collect_cycles();
    
    sleep(std::time::Duration::from_secs(10));
    
}