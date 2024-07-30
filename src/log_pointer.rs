use std::ptr::NonNull;
use std::sync::{LockResult, Mutex, MutexGuard, TryLockResult};

use crate::cc::CcBox;
use crate::Trace;

pub struct ObjectCopy {
    copy: Vec<NonNull<CcBox<()>>>,
}


pub struct LogPointer {
    mutex: Mutex<Option<ObjectCopy>>, //TODO usa un atomic pointer al posto del mutex
}

unsafe impl Send for LogPointer {}

unsafe impl Sync for LogPointer {}

impl LogPointer {
    pub fn new() -> Self {
        Self {
            mutex: Mutex::new(None),
        }
    }
}

pub struct LoggedMutex<T: Trace + ?Sized + 'static> {
    log_pointer: LogPointer,
    mutex: Mutex<T>,
}

impl<T: Trace> LoggedMutex<T> {
    fn new(value: T) -> Self {
        Self {
            log_pointer: LogPointer::new(),
            mutex: Mutex::new(value),
        }
    }
}

impl<T: Trace + ?Sized> LoggedMutex<T> {
    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        self.mutex.lock()
    }

    pub fn try_lock(&self) -> TryLockResult<MutexGuard<'_, T>> {
        self.mutex.try_lock()
    }

    pub fn is_poisoned(&self) -> bool {
        self.mutex.is_poisoned()
    }

    pub fn clear_poison(&self) {
        self.mutex.clear_poison()
    }

    pub fn into_inner(self) -> LockResult<T>
    where
        T: Sized,
    {
        self.mutex.into_inner()
    }

    pub fn get_mut(&mut self) -> LockResult<&mut T> {

        //se hai già una referenza mutabile, elimina il logpoiner perchè è già stata fatta la copia da un mutex che contiene questo mutex (all'interno del CcBox)
        { *self.log_pointer.mutex.lock().unwrap() = None; }
        
        self.mutex.get_mut()
    }
}