use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::{collect, COLLECTOR, POSSIBLE_CYCLES};
use crate::cc::CONDVAR;
use crate::state::try_state;

pub fn init_collector() {
    let c = CONDVAR.get_or_init(|| Arc::new(Condvar::new()));
    let _ = COLLECTOR.get_or_init(|| {
        thread::spawn(|| {
            let m: Mutex<()> = Mutex::new(());
            loop {
                { let _s = c.wait(m.lock().unwrap()); }

                //if someone awaken me then I will collect
                println!("collecting");
                let _ = try_state(|state| {
                    if state.is_collecting() {
                        return;
                    }

                    let _ = POSSIBLE_CYCLES.try_with(|pc| {
                        collect(state, pc);
                    });
                });
            }
        })
    });
}