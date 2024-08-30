use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use rust_cc::{Cc, collect_cycles};
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
        let n: Cc<RefCell<u64>> = Cc::new(RefCell::new(0));
        loop {
            *n.borrow_mut() += 1;
            if *EXIT.lock().unwrap() {
                drop(n); //FIXME anche se droppo da memory leak
                break;
            }
        }
    });

    sleep(Duration::from_millis(100));
    collect_and_stop();
    assert_eq!(N_ACYCLIC_DROPPED.load(Ordering::Relaxed), 1);
    assert_eq!(N_CYCLIC_DROPPED.load(Ordering::Relaxed), 0);
    *EXIT.lock().unwrap() = true;
    let _ = t.join();
}

#[test]
fn collector_version_check() {
    initial_test();

    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 0);
    collect_and_stop();
    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 1);
}

#[test]
fn log_version_check() {
    initial_test();

    let l = LoggedMutex::new(0);
    collect_cycles();


    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 0);
    collect_and_stop();
    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 1);
}

//TODO controlla le thread actions, aggiungilo sopra

//TODO cosa succede un loggedmutex interno al loggedmutex

//TODO dopo che hai fatto il lock di logpointer, controlla che ha fatto la copia, idem per try_lock

fn initial_test() {
    assert!(COLLECTOR.get().is_none());
    assert!(CONDVAR.get().is_none());
    assert_eq!(*COLLECTOR_STATE.lock().unwrap(), STATES::SLEEPING);
    assert_eq!(COLLECTOR_VERSION.load(Ordering::Relaxed), 0);
}


