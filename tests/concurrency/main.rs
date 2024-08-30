use std::sync::atomic::Ordering;
use std::thread;

use rust_cc::Cc;
use rust_cc::collector::{collect_and_stop, COLLECTOR, COLLECTOR_STATE, COLLECTOR_VERSION, CONDVAR, N_ACYCLIC_DROPPED, N_CYCLIC_DROPPED, STATES};
use rust_cc::log_pointer::LoggedMutex;

use crate::cyclic::Cyclic;

mod cyclic;

#[test]
fn collect_acyclic() {
    print();
    let t = thread::spawn(|| {
        let _ = Cc::new(4);
    });

    let _ = Cc::new(4);


    let _ = t.join();

    collect_and_stop();

    assert_eq!(N_ACYCLIC_DROPPED.load(Ordering::Relaxed), 2);
    assert_eq!(N_CYCLIC_DROPPED.load(Ordering::Relaxed), 0);
}

#[test]
fn collect_cyclic() {
    print();
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

fn print() {
    assert!(COLLECTOR.get().is_none());
    assert!(CONDVAR.get().is_none());
    assert_eq!(*COLLECTOR_STATE.lock().unwrap(), STATES::SLEEPING);
    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 0);
}


