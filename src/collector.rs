use std::collections::HashSet;
use std::mem::swap;
use std::ptr::NonNull;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

use crate::{collect, THREAD_ACTIONS};
use crate::cc::{Action, ActionEntry, CcBox};
use crate::counter_marker::Mark;
use crate::list::{CountedList, ListMethods};
use crate::state::{replace_state_field, State, try_state};
use crate::utils::cc_dealloc;

pub static COLLECTOR: OnceLock<thread::JoinHandle<()>> = OnceLock::new();

//usage:  CONDVAR.get().clone().unwrap().notify_one();
pub static CONDVAR: OnceLock<Arc<Condvar>> = OnceLock::new();

pub fn init_collector() {
    let c = CONDVAR.get_or_init(|| Arc::new(Condvar::new()));
    let _ = COLLECTOR.get_or_init(|| {
        thread::spawn(|| {
            let mut possible_cycles: CountedList = CountedList::new();

            let m: Mutex<()> = Mutex::new(());
            loop {
          
                { let _s = c.wait(m.lock().unwrap()); }
       
                //if someone awaken me then I will collect

                let _ = try_state(|state| {
                    if state.is_collecting() {
                        return;
                    }

                    //prima cosa incremento tutte le reference

                    let mut void = THREAD_ACTIONS.lock().unwrap();

                    let mut changes: Vec<ActionEntry> = Vec::new();
                    
                    swap(&mut *void, &mut changes);
                    
                    for a in &changes {
                        unsafe {
                            if let Action::Add = a.action {
                                println!("{}", a.cc_box.as_ref().counter_marker().counter());
                                if a.cc_box.as_ref().counter_marker().increment_counter().is_err() {
                                    panic!("Too many references has been created to a single Cc");
                                }
                                println!("{}", a.cc_box.as_ref().counter_marker().counter())
                            }
                        }
                    }


                    //seconda cosa decremento tutte le reference

                    for a in &changes {
                        unsafe {
                            if let Action::Remove = a.action {
                                println!("{}", a.cc_box.as_ref().counter_marker().counter());

                                let _ = a.cc_box.as_ref().counter_marker().decrement_counter();


                                println!("{}", a.cc_box.as_ref().counter_marker().counter());

                                if a.cc_box.as_ref().counter_marker().counter() == 0 {
                                    println!("qualcosa sta venendo deallocato");
                                    
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
            }
        })
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

        cc_dealloc(element.cast::<CcBox<()>>(), layout, state);

        // _dropping_guard is dropped here, resetting state.dropping
    }
}

#[inline]
pub(crate) fn remove_from_list(ptr: NonNull<CcBox<()>>, possible_cycles: &mut CountedList) {
    let counter_marker = unsafe { ptr.as_ref() }.counter_marker();

    // Check if ptr is in possible_cycles list
    if counter_marker.is_in_possible_cycles() {
        // ptr is in the list, remove it

        let mut list = possible_cycles;
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

    let mut list = possible_cycles;
    
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