use std::sync::atomic::Ordering;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Mutex;
use std::thread;

use rust_cc::{Cc, collect_cycles};
use rust_cc::cc::THREAD_ACTIONS;
use rust_cc::collector::{collect_and_stop, COLLECTOR, COLLECTOR_STATE, COLLECTOR_VERSION, CONDVAR, N_ACYCLIC_DROPPED, N_CYCLIC_DROPPED, STATES};
use rust_cc::log_pointer::LoggedMutex;

use crate::cyclic::Cyclic;

mod cyclic;

#[test]
fn collect_acyclic() {
    initial_test();
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
    initial_test();
    let t = thread::spawn(|| {
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


#[test]
fn collect_cyclic_move() {
    initial_test();
    let _cyclic1 = Cc::new(Cyclic {
        cyclic: LoggedMutex::new(None),
    });
    let t = thread::spawn(move || {
        let _cyclic2 = Cc::new(Cyclic {
            cyclic: LoggedMutex::new(None),
        });

        let _cyclic3 = Cc::new(Cyclic {
            cyclic: LoggedMutex::new(None),
        });

        *_cyclic1.cyclic.lock().unwrap() = Some(_cyclic2.clone());
        *_cyclic2.cyclic.lock().unwrap() = Some(_cyclic3.clone());
        *_cyclic3.cyclic.lock().unwrap() = Some(_cyclic1.clone());
    });
    t.join().unwrap();

    collect_and_stop();

    assert_eq!(N_ACYCLIC_DROPPED.load(Ordering::Relaxed), 0);
    assert_eq!(N_CYCLIC_DROPPED.load(Ordering::Relaxed), 3);
}

#[test]
fn more_thread() {
    initial_test();
    let mut t = Vec::new();
    for _ in 0..100 {
        t.push(thread::spawn(|| {
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
        }));
    }

    for j in t {
        j.join().unwrap();
    }

    collect_and_stop();

    assert_eq!(N_ACYCLIC_DROPPED.load(Ordering::Relaxed), 0);
    assert_eq!(N_CYCLIC_DROPPED.load(Ordering::Relaxed), 3 * 100);
}

static EXIT: Mutex<bool> = Mutex::new(false);

#[test]
fn no_dealloc() {
    initial_test();

    { let _ = Cc::new(4); }

    let t = thread::spawn(|| {
        let n: LoggedMutex<Cc<u64>> = LoggedMutex::new(Cc::new(0));
        loop {
            let l = n.lock().unwrap();
            if *EXIT.lock().unwrap() {
                break;
            }
            drop(l);
        }
    });

    collect_and_stop();
    assert_eq!(N_ACYCLIC_DROPPED.load(Relaxed), 1);
    assert_eq!(N_CYCLIC_DROPPED.load(Relaxed), 0);
    *EXIT.lock().unwrap() = true;
    let _ = t.join();
}

#[test]
fn collector_version_check() {
    initial_test();
    { let _ = Cc::new(4); }
    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 0);
    collect_and_stop();
    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 1);
}

#[test]
fn log_version_check() {
    initial_test();
    { let _ = Cc::new(4); }

    {
        let l = LoggedMutex::new(Cc::new(0));
        assert_eq!(l.log_pointer.version.load(Relaxed), 0);
        loop {
            collect_cycles();
            let f = l.lock().unwrap();
            drop(f);
            if l.log_pointer.version.load(Relaxed) > 0 {
                break;
            }
        }
    }

    collect_and_stop();
}

#[test]
fn check_thread_actions() {
    initial_test();

    {
        let _ = Cc::new(4); //just one REMOVE is counted
    }

    assert_eq!(THREAD_ACTIONS.lock().unwrap().len(), 1);

    {
        let g = Cc::new(4);
        let _ = g.clone(); //ADD
    } //2 REMOVE

    assert_eq!(THREAD_ACTIONS.lock().unwrap().len(), 4);

    collect_and_stop();
}

fn initial_test() {
    assert!(COLLECTOR.get().is_none());
    assert!(CONDVAR.get().is_none());
    assert_eq!(*COLLECTOR_STATE.lock().unwrap(), STATES::SLEEPING);
    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 0);
}


