use alloc::alloc::Layout;
#[cfg(feature = "nightly")]
use core::{
    marker::Unsize,
    ops::CoerceUnsized,
    ptr::{DynMetadata, metadata},
};
use core::cell::UnsafeCell;
use core::ops::Deref;
use core::ptr::{self, drop_in_place, NonNull};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::collector::{COLLECTOR, init_collector, remove_from_list};
use crate::counter_marker::{CounterMarker, Mark};
use crate::list::ListMethods;
use crate::state::{state, State};
use crate::trace::{Context, ContextInner, CopyContext, Finalize, Trace};
use crate::utils::*;

pub enum Action {
    Add,
    Remove,
}

pub(crate) struct ActionEntry {
    pub(crate) cc_box: NonNull<CcBox<()>>,
    pub(crate) action: Action,
}

unsafe impl Send for ActionEntry {}

impl ActionEntry {
    fn new(cc_box: NonNull<CcBox<()>>, action: Action) -> Self {
        Self {
            cc_box,
            action,
        }
    }
}


pub(crate) static THREAD_ACTIONS: Mutex<Vec<ActionEntry>> = Mutex::new(Vec::new());


/// A thread-local cycle collected pointer.
///
/// See the [module-level documentation][`mod@crate`] for more details.
#[repr(transparent)]
pub struct Cc<T: ?Sized + Trace + 'static> {
    inner: NonNull<CcBox<T>>,
    _phantom: PhantomData<Arc<T>>, // Make Cc !Send and !Sync
}

#[cfg(feature = "nightly")]
impl<T, U> CoerceUnsized<Cc<U>> for Cc<T>
where
    T: ?Sized + Trace + Unsize<U> + 'static,
    U: ?Sized + Trace + 'static,
{}


unsafe impl<T: Trace + 'static + Send> Send for Cc<T> {}

unsafe impl<T: Trace + 'static + Sync> Sync for Cc<T> {}

impl<T: Trace + 'static> Cc<T> {
    /// Creates a new `Cc`.
    ///
    /// # Collection
    ///
    /// This method may start a collection when the `auto-collect` feature is enabled.
    ///
    /// See the [`config` module documentation][`mod@crate::config`] for more details.
    ///
    /// # Panics
    ///
    /// Panics if the automatically-stared collection panics.
    #[inline(always)]
    #[must_use = "newly created Cc is immediately dropped"]
    #[track_caller]
    pub fn new(t: T) -> Cc<T> {
        init_collector();

        state(|state| {
            #[cfg(debug_assertions)]
            if state.is_tracing() {
                panic!("Cannot create a new Cc while tracing!");
            }

            #[cfg(feature = "auto-collect")]
            super::trigger_collection();

            Cc {
                inner: CcBox::new(t, state),
                _phantom: PhantomData,
            }
        })
    }

    /*
       /// Takes out the value inside a [`Cc`].
       ///
       /// # Panics
       /// Panics if the [`Cc`] is not unique (see [`is_unique`]).
       ///
       /// [`is_unique`]: fn@Cc::is_unique
      #[ine]
       #[track_caller]
       pub fn into_inner(self) -> T {
           assert!(self.is_unique(), "Cc<_> is not unique");
    
           assert!(
               !self.counter_marker().is_traced(),
               "Cc<_> is being used by the collector and inner value cannot be taken out (this might have happen inside Trace, Finalize or Drop implementations)."
           );
    
           // Make sure self is not into POSSIBLE_CYCLES before deallocating
           remove_from_list(self.inner.cast());
    
           // SAFETY: self is unique and is not inside any list
           unsafe {
               let t = ptr::read(self.inner().get_elem());
               let layout = self.inner().layout();
               let _ = try_state(|state| cc_dealloc(self.inner, layout, state));
               mem::forget(self); // Don't call drop on this Cc
               t
           }
       }*/
}

impl<T: ?Sized + Trace + 'static> Cc<T> {
    /// Returns `true` if the two [`Cc`]s point to the same allocation. This function ignores the metadata of `dyn Trait` pointers.
    #[inline]
    pub fn ptr_eq(this: &Cc<T>, other: &Cc<T>) -> bool {
        ptr::eq(this.inner.as_ptr() as *const (), other.inner.as_ptr() as *const ())
    }

    /// Returns the number of [`Cc`]s to the pointed allocation.
    #[inline]
    pub fn strong_count(&self) -> u32 {
        self.counter_marker().counter()
    }

    /// Returns `true` if the strong reference count is `1`, `false` otherwise.
    #[inline]
    pub fn is_unique(&self) -> bool {
        self.strong_count() == 1
    }

    /// Makes the value in the managed allocation finalizable again.
    ///
    /// # Panics
    ///
    /// Panics if called during a collection.
    #[cfg(feature = "finalization")]
    #[inline]
    #[track_caller]
    pub fn finalize_again(&mut self) {
        // The is_finalizing and is_dropping checks are necessary to avoid letting this function
        // be called from Cc::drop implementation, since it doesn't set is_collecting to true
        assert!(
            state(|state| !state.is_collecting() && !state.is_finalizing() && !state.is_dropping()),
            "Cc::finalize_again cannot be called while collecting"
        );

        self.counter_marker().set_finalized(false);
    }

    /// Returns `true` if the value in the managed allocation has already been finalized, `false` otherwise.
    #[cfg(feature = "finalization")]
    #[inline]
    pub fn already_finalized(&self) -> bool {
        !self.counter_marker().needs_finalization()
    }

    /// Marks the managed allocation as *alive*.
    ///
    /// Every time a [`Cc`] is dropped, the pointed allocation is buffered to be processed in the next collection.
    /// This method simply removes the managed allocation from the buffer, potentially reducing the amount of work
    /// needed to be done by the collector.
    ///
    /// This method is a no-op when called on a [`Cc`] pointing to an allocation which is not buffered.
    #[inline]
    pub fn mark_alive(&self) {
        //   remove_from_list(self.inner.cast());
    }

    #[inline(always)]
    fn counter_marker(&self) -> &CounterMarker {
        &self.inner().counter_marker
    }

    #[inline(always)]
    pub(crate) fn inner(&self) -> &CcBox<T> {
        unsafe { self.inner.as_ref() }
    }

    #[cfg(feature = "weak-ptr")]
    #[inline(always)]
    pub(crate) fn inner_ptr(&self) -> NonNull<CcBox<T>> {
        self.inner
    }


    #[inline(always)]
    #[must_use]
    pub(crate) fn __new_internal(inner: NonNull<CcBox<T>>) -> Cc<T> {
        Cc {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl<T: ?Sized + Trace + 'static> Clone for Cc<T> {
    /// Makes a clone of the [`Cc`] pointer.
    ///
    /// This creates another pointer to the same allocation, increasing the strong reference count.
    ///
    /// Cloning a [`Cc`] also marks the managed allocation as `alive`. See [`mark_alive`][`Cc::mark_alive`] for more details.
    ///
    /// # Panics
    ///
    /// Panics if the strong reference count exceeds the maximum supported.
    #[inline]
    #[track_caller]
    fn clone(&self) -> Self {
        #[cfg(debug_assertions)]
        if state(|state| state.is_tracing()) {
            panic!("Cannot clone while tracing!");
        }


        THREAD_ACTIONS.lock().unwrap().push(ActionEntry::new(self.inner.cast(), Action::Add));


        /* if self.counter_marker().increment_counter().is_err() {
             panic!("Too many references has been created to a single Cc");
         }*/

        //self.mark_alive();

        // It's always safe to clone a Cc
        Cc {
            inner: self.inner,
            _phantom: PhantomData,
        }
    }
}


impl<T: ?Sized + Trace + 'static> Deref for Cc<T> {
    type Target = T;

    #[inline]
    #[track_caller]
    fn deref(&self) -> &Self::Target {
        #[cfg(debug_assertions)]
        if state(|state| state.is_tracing()) {
            panic!("Cannot deref while tracing!");
        }

        //self.mark_alive();

        self.inner().get_elem()
    }
}

impl<T: ?Sized + Trace + 'static> Drop for Cc<T> {
    fn drop(&mut self) {
        let v = COLLECTOR.get().unwrap().lock().unwrap();
        let Some(f) = &*v else { return; };
        let id = f.thread().id();
        drop(v);
        if !id.eq(&thread::current().id()) {
            THREAD_ACTIONS.lock().unwrap().push(ActionEntry::new(self.inner.cast(), Action::Remove));
        }
    }
}

unsafe impl<T: ?Sized + Trace + 'static> Trace for Cc<T> {
    #[inline]
    #[track_caller]
    fn trace(&self, ctx: &mut Context<'_>) {
        if CcBox::trace(self.inner.cast(), ctx) {
            self.inner().get_elem().trace(ctx);
        }
    }

    #[inline]
    #[track_caller]
    fn make_copy(&mut self, ctx: &mut CopyContext<'_>) {
        ctx.copy_vec.push(self.inner.cast());
    }
}

impl<T: ?Sized + Trace + 'static> Finalize for Cc<T> {}

#[repr(C)]
pub(crate) struct CcBox<T: ?Sized + Trace + 'static> {
    next: UnsafeCell<Option<NonNull<CcBox<()>>>>,
    prev: UnsafeCell<Option<NonNull<CcBox<()>>>>,

    #[cfg(feature = "nightly")]
    vtable: DynMetadata<dyn InternalTrace>,

    #[cfg(not(feature = "nightly"))]
    fat_ptr: NonNull<dyn InternalTrace>,

    counter_marker: CounterMarker,
    _phantom: PhantomData<Arc<()>>, // Make CcBox !Send and !Sync

    // This UnsafeCell is necessary, since we want to execute Drop::drop (which takes an &mut)
    // for elem but still have access to the other fields of CcBox
    elem: UnsafeCell<T>,
}

impl<T: Trace + 'static> CcBox<T> {
    #[inline(always)]
    #[must_use]
    fn new(t: T, state: &State) -> NonNull<CcBox<T>> {
        let layout = Layout::new::<CcBox<T>>();

        #[cfg(feature = "finalization")]
        let already_finalized = state.is_finalizing();
        #[cfg(not(feature = "finalization"))]
        let already_finalized = false;

        unsafe {
            let ptr: NonNull<CcBox<T>> = cc_alloc(layout, state);
            ptr::write(
                ptr.as_ptr(),
                CcBox {
                    next: UnsafeCell::new(None),
                    prev: UnsafeCell::new(None),
                    #[cfg(feature = "nightly")]
                    vtable: metadata(ptr.as_ptr() as *mut dyn InternalTrace),
                    #[cfg(not(feature = "nightly"))]
                    fat_ptr: NonNull::new_unchecked(ptr.as_ptr() as *mut dyn InternalTrace),
                    counter_marker: CounterMarker::new_with_counter_to_one(already_finalized),
                    _phantom: PhantomData,
                    elem: UnsafeCell::new(t),
                },
            );
            ptr
        }
    }

    #[inline(always)]
    #[cfg(all(test, feature = "std"))] // Only used in unit tests
    #[must_use]
    pub(crate) fn new_for_tests(t: T) -> NonNull<CcBox<T>> {
        state(|state| CcBox::new(t, state))
    }
}

impl<T: ?Sized + Trace + 'static> CcBox<T> {
    #[inline]
    pub(crate) fn get_elem(&self) -> &T {
        unsafe { &*self.elem.get() }
    }

    #[inline]
    pub(crate) fn get_elem_mut(&self) -> *mut T {
        self.elem.get()
    }

    #[inline]
    pub(crate) fn counter_marker(&self) -> &CounterMarker {
        &self.counter_marker
    }

    #[inline]
    pub(crate) fn layout(&self) -> Layout {
        #[cfg(feature = "nightly")]
        {
            self.vtable.layout()
        }

        #[cfg(not(feature = "nightly"))]
        unsafe {
            Layout::for_value(self.fat_ptr.as_ref())
        }
    }

    #[inline]
    pub(super) fn get_next(&self) -> *mut Option<NonNull<CcBox<()>>> {
        self.next.get()
    }

    #[inline]
    pub(super) fn get_prev(&self) -> *mut Option<NonNull<CcBox<()>>> {
        self.prev.get()
    }
}

unsafe impl<T: ?Sized + Trace + 'static> Trace for CcBox<T> {
    #[inline(always)]
    fn trace(&self, ctx: &mut Context<'_>) {
        self.get_elem().trace(ctx);
    }

    fn make_copy(&mut self, _: &mut CopyContext<'_>) {
        unreachable!();
    }
}

impl<T: ?Sized + Trace + 'static> Finalize for CcBox<T> {
    #[inline(always)]
    fn finalize(&self) {
        self.get_elem().finalize();
    }
}


// Functions in common between every CcBox<_>
impl CcBox<()> {
    #[inline]
    pub(super) fn trace_inner(ptr: NonNull<Self>, ctx: &mut Context<'_>) {
        unsafe {
            CcBox::get_traceable(ptr).as_ref().trace(ctx);
        }
    }

    #[cfg(feature = "finalization")]
    #[inline]
    pub(super) fn finalize_inner(ptr: NonNull<Self>) -> bool {
        unsafe {
            if ptr.as_ref().counter_marker().needs_finalization() {
                // Set finalized
                ptr.as_ref().counter_marker().set_finalized(true);

                CcBox::get_traceable(ptr).as_ref().finalize_elem();
                true
            } else {
                false
            }
        }
    }

    /// SAFETY: `drop_in_place` conditions must be true.
    #[inline]
    pub(super) unsafe fn drop_inner(ptr: NonNull<Self>) {
        CcBox::get_traceable(ptr).as_mut().drop_elem();
    }

    #[inline]
    fn get_traceable(ptr: NonNull<Self>) -> NonNull<dyn InternalTrace> {
        #[cfg(feature = "nightly")]
        unsafe {
            let vtable = ptr.as_ref().vtable;
            NonNull::from_raw_parts(ptr.cast(), vtable)
        }

        #[cfg(not(feature = "nightly"))]
        unsafe {
            ptr.as_ref().fat_ptr
        }
    }

    pub(super) fn start_tracing(ptr: NonNull<Self>, ctx: &mut Context<'_>) {
        let counter_marker = unsafe { ptr.as_ref() }.counter_marker();
        match ctx.inner() {
            ContextInner::Counting { root_list, .. } => {
                // ptr is NOT into POSSIBLE_CYCLES list: ptr has just been removed from
                // POSSIBLE_CYCLES by rust_cc::collect() (see lib.rs) before calling this function

                root_list.add(ptr);

                // Reset trace_counter
                counter_marker.reset_tracing_counter();

                // Element is surely not already marked, marking
                counter_marker.mark(Mark::Traced);
            }
            ContextInner::RootTracing { .. } => {
                // ptr is a root

                // Nothing to do here, ptr is already unmarked
                debug_assert!(counter_marker.is_not_marked());
            }
        }

        // ptr is surely to trace
        //
        // This function is called from collect_cycles(), which doesn't know the
        // exact type of the element inside CcBox, so trace it using the vtable
        CcBox::trace_inner(ptr, ctx);
    }

    /// Returns whether `ptr.elem` should be traced.
    ///
    /// This function returns a `bool` instead of directly tracing the element inside the CcBox, since this way
    /// we can avoid using the vtable most of the times (the responsibility of tracing the inner element is passed
    /// to the caller, which *might* have more information on the type inside CcBox than us).
    #[inline(never)] // Don't inline this function, it's huge
    #[must_use = "the element inside ptr is not traced by CcBox::trace"]
    fn trace(ptr: NonNull<Self>, ctx: &mut Context<'_>) -> bool {
        #[inline(always)]
        fn non_root(counter_marker: &CounterMarker) -> bool {
            counter_marker.tracing_counter() == counter_marker.counter()
        }

        let counter_marker = unsafe { ptr.as_ref() }.counter_marker();
        match ctx.inner() {
            ContextInner::Counting {
                possible_cycles,
                root_list,
                non_root_list,
            } => {
                if !counter_marker.is_traced() {
                    // Not already marked

                    // Make sure ptr is not in POSSIBLE_CYCLES list
                    remove_from_list(ptr, possible_cycles);

                    counter_marker.reset_tracing_counter();
                    let res = counter_marker.increment_tracing_counter();
                    debug_assert!(res.is_ok());

                    // Check invariant (tracing_counter is always less or equal to counter)
                    debug_assert!(counter_marker.tracing_counter() <= counter_marker.counter());

                    if non_root(counter_marker) {
                        non_root_list.add(ptr);
                    } else {
                        root_list.add(ptr);
                    }

                    // Marking here since the previous debug_asserts might panic
                    // before ptr is actually added to root_list or non_root_list
                    counter_marker.mark(Mark::Traced);

                    // Continue tracing
                    true
                } else {
                    // Check counters invariant (tracing_counter is always less or equal to counter)
                    // Only < is used here since tracing_counter will be incremented (by 1)
                    debug_assert!(counter_marker.tracing_counter() < counter_marker.counter());

                    let res = counter_marker.increment_tracing_counter();
                    debug_assert!(res.is_ok());

                    if non_root(counter_marker) {
                        // Already marked, so ptr was put in root_list
                        root_list.remove(ptr);
                        non_root_list.add(ptr);
                    }

                    // Don't continue tracing
                    false
                }
            }
            ContextInner::RootTracing { non_root_list, root_list } => {
                if counter_marker.is_traced() {
                    // Marking NonMarked since ptr will be removed from any list it's into. Also, marking
                    // NonMarked will avoid tracing this CcBox again (thanks to the if condition)
                    counter_marker.mark(Mark::NonMarked);

                    if non_root(counter_marker) {
                        non_root_list.remove(ptr);
                    } else {
                        root_list.remove(ptr);
                    }

                    // Continue root tracing
                    true
                } else {
                    // Don't continue tracing
                    false
                }
            }
        }
    }
}

// Trait used to make it possible to drop/finalize only the elem field of CcBox
// and without taking a &mut reference to the whole CcBox
trait InternalTrace: Trace {
    #[cfg(feature = "finalization")]
    fn finalize_elem(&self);

    /// Safety: see `drop_in_place`
    unsafe fn drop_elem(&self);
}

impl<T: ?Sized + Trace + 'static> InternalTrace for CcBox<T> {
    #[cfg(feature = "finalization")]
    fn finalize_elem(&self) {
        self.get_elem().finalize();
    }

    unsafe fn drop_elem(&self) {
        drop_in_place(self.get_elem_mut());
    }
}
