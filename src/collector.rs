use std::{mem, thread};
use std::collections::HashSet;
use std::mem::swap;
use std::ptr::NonNull;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use STATES::COLLECTING;

use crate::{collect, collect_cycles};
use crate::cc::{Action, ActionEntry, CcBox, THREAD_ACTIONS};
use crate::counter_marker::Mark;
use crate::list::{CountedList, ListMethods};
use crate::log_pointer::ObjectCopy;
use crate::state::{replace_state_field, State, try_state};
use crate::utils::cc_dealloc;

pub static COLLECTOR: OnceLock<Mutex<Option<thread::JoinHandle<()>>>> = OnceLock::new();

//usage:  CONDVAR.get().clone().unwrap().notify_one();
pub static CONDVAR: OnceLock<Condvar> = OnceLock::new();

pub static COLLECTOR_STATE: Mutex<STATES> = Mutex::new(STATES::SLEEPING);

pub static COLLECTOR_VERSION: AtomicU64 = AtomicU64::new(0);

pub static LOGS: Mutex<Vec<Arc<ObjectCopy>>> = Mutex::new(Vec::new());

pub static STOP: AtomicBool = AtomicBool::new(false);

pub static N_ACYCLIC_DROPPED: AtomicU64 = AtomicU64::new(0);

pub static N_CYCLIC_DROPPED: AtomicU64 = AtomicU64::new(0);

pub fn is_collecting() -> bool {
    let state = COLLECTOR_STATE.lock().unwrap();
    match *state {
        COLLECTING => true,
        _ => false,
    }
}

pub fn init_collector() {
    let c = CONDVAR.get_or_init(Condvar::new);
    let _ = COLLECTOR.get_or_init(|| {
        let t = thread::spawn(|| {
            let mut possible_cycles: CountedList = CountedList::new();
            let m: Mutex<()> = Mutex::new(());

            loop {
                { let _s = c.wait(m.lock().unwrap()); }

                {
                    let mut state = COLLECTOR_STATE.lock().unwrap();
                    *state = COLLECTING;

                    COLLECTOR_VERSION.fetch_add(1, Ordering::AcqRel);
                }

                //if someone awaken me then I will collect

                let _ = try_state(|state| {
                    if state.is_collecting() {
                        return;
                    }

                    //prima cosa incremento tutte le reference
                    let mut changes: Vec<ActionEntry> = Vec::new();
                    {
                        let mut void = THREAD_ACTIONS.lock().unwrap();

                        swap(&mut *void, &mut changes);
                    }

                    for a in &changes {
                        unsafe {
                            if let Action::Add = a.action {
                                if a.cc_box.as_ref().counter_marker().increment_counter().is_err() {
                                    panic!("Too many references has been created to a single Cc");
                                }
                            }
                        }
                    }

                    //seconda cosa decremento tutte le reference

                    for a in &changes {
                        unsafe {
                            if let Action::Remove = a.action {
                                let _ = a.cc_box.as_ref().counter_marker().decrement_counter();


                                if a.cc_box.as_ref().counter_marker().counter() == 0 {
                                    N_ACYCLIC_DROPPED.fetch_add(1, Ordering::Relaxed);
                                    drop_elem(a.cc_box, state, &mut possible_cycles)
                                } else {
                                    //lo marchiamo come possibile ciclo
                                    add_to_list(a.cc_box, &mut possible_cycles);
                                }
                            }
                        }
                    }

                    //lista dei possible cycles preso dalle azioni effettuate
                    let mut ps = HashSet::new();

                    for a in &changes {
                        ps.insert(a.cc_box);
                    }

                    collect(state, &mut possible_cycles);
                });

                {
                    let mut state = COLLECTOR_STATE.lock().unwrap();
                    *state = STATES::CLEANING;
                }

                if let Ok(mut l) = LOGS.lock() {
                    for x in &*l {
                        mem::swap(&mut *x.copy.borrow_mut(), &mut Vec::new());
                    }

                    mem::swap(&mut *l, &mut Vec::new());
                }

                {
                    let mut state = COLLECTOR_STATE.lock().unwrap();
                    *state = STATES::SLEEPING;
                }

                if STOP.load(Ordering::Acquire) {
                    break;
                }
            }
        });
        return Mutex::new(Some(t));
    });
}

fn drop_elem(element: NonNull<CcBox<()>>, state: &State, possible_cycles: &mut CountedList) {
    unsafe {
        remove_from_list(element, possible_cycles);

        let _dropping_guard = replace_state_field!(dropping, true, state);
        let layout = element.as_ref().layout();

        /*  // Set the object as dropped before dropping and deallocating it
          // This feature is used only in weak pointers, so do this only if they're enabled
          #[cfg(feature = "weak-ptr")]
          {
              self.counter_marker().set_dropped(true);
          }*/

        // SAFETY: we're the only one to have a pointer to this allocation

        CcBox::drop_inner(element.cast());


        #[cfg(feature = "pedantic-debug-assertions")]
        debug_assert_eq!(
            0, element.as_ref().counter_marker().counter(),
            "Trying to deallocate a CcBox with a reference counter > 0"
        );

        cc_dealloc(element.cast::<CcBox<()>>(), layout);

        // _dropping_guard is dropped here, resetting state.dropping
    }
}

#[inline]
pub(crate) fn remove_from_list(ptr: NonNull<CcBox<()>>, possible_cycles: &mut CountedList) {
    let counter_marker = unsafe { ptr.as_ref() }.counter_marker();

    // Check if ptr is in possible_cycles list
    if counter_marker.is_in_possible_cycles() {
        // ptr is in the list, remove it

        let list = possible_cycles;
        // Confirm is_in_possible_cycles() in debug builds
        #[cfg(feature = "pedantic-debug-assertions")]
        debug_assert!(list.contains(ptr));

        counter_marker.mark(Mark::NonMarked);
        list.remove(ptr);
    } else {
        // ptr is not in the list

        // Confirm !is_in_possible_cycles() in debug builds.
        // This is safe to do since we're not putting the CcBox into the list
        #[cfg(feature = "pedantic-debug-assertions")]
        debug_assert! {
            !possible_cycles.contains(ptr)
        };
    }
}


#[inline]
pub(crate) fn add_to_list(ptr: NonNull<CcBox<()>>, possible_cycles: &mut CountedList) {
    let counter_marker = unsafe { ptr.as_ref() }.counter_marker();

    let list = possible_cycles;

    // Check if ptr is in possible_cycles list since we have to move it at its start
    if counter_marker.is_in_possible_cycles() {
        // Confirm is_in_possible_cycles() in debug builds
        #[cfg(feature = "pedantic-debug-assertions")]
        debug_assert!(list.contains(ptr));

        list.remove(ptr);
        // In this case we don't need to update the mark since we put it back into the list
    } else {
        // Confirm !is_in_possible_cycles() in debug builds
        #[cfg(feature = "pedantic-debug-assertions")]
        debug_assert!(!list.contains(ptr));
        debug_assert!(counter_marker.is_not_marked());

        // Mark it
        counter_marker.mark(Mark::PossibleCycles);
    }
    // Add to the list
    //
    // Make sure this operation is the first after the if-else, since the CcBox is in
    // an invalid state now (it's marked Mark::PossibleCycles, but it isn't into the list)
    list.add(ptr);
}

pub fn collect_and_stop() {
    let mut a = COLLECTOR.get().unwrap().lock().unwrap();

    let o = a.take();
    drop(a);
    if let Some(o) = o {
        STOP.store(true, Ordering::Release);

        collect_cycles();

        let _ = o.join();
    }
}

pub enum STATES {
    SLEEPING,
    COLLECTING,
    CLEANING,
}