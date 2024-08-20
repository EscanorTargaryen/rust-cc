use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::sync::{LockResult, Mutex, MutexGuard, TryLockResult};

use crate::{Cc, Context, Finalize, Trace};
use crate::cc::CcBox;
use crate::trace::CopyContext;

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
    pub fn new(value: T) -> Self {
        Self {
            log_pointer: LogPointer::new(),
            mutex: Mutex::new(value),
        }
    }
}

impl<T: Trace + ?Sized> LoggedMutex<T> {
    fn log_copy<E>(&self, result: &mut Result<MutexGuard<'_, T>, E>) {
        if let Ok(result) = result {
            let mut log = self.log_pointer.mutex.lock().unwrap();
            if log.is_none() {
                let mut vec = Vec::new();
                let mut ctx = CopyContext::new(&mut vec);
                result.make_copy(&mut ctx);
                let obj = ObjectCopy {
                    copy: vec,
                };

                log.replace(obj);
            }
        }
    }


    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        let mut result = self.mutex.lock();

        self.log_copy(&mut result);

        result
    }

    pub fn try_lock(&self) -> TryLockResult<MutexGuard<'_, T>> {
        let mut result = self.mutex.try_lock();

        self.log_copy(&mut result);

        result
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
} //FIXME crea un logpoiner solo se si sta collezionando

unsafe impl<T: Trace + ?Sized> Trace for LoggedMutex<T> {
    fn trace(&self, ctx: &mut Context<'_>) {
        let object = self.log_pointer.mutex.lock().unwrap();

        if let Some(obj) = &*object {
            obj.copy.iter().map(|el| {
                ManuallyDrop::new(Cc::__new_internal(*el))
            }).for_each(|el| {
                el.trace(ctx);
            });

            return;
        } else {
            drop(object);

            if let Ok(r) = self.mutex.try_lock() {
                r.trace(ctx);
            } else {
                let object = self.log_pointer.mutex.lock().unwrap();

                if let Some(obj) = &*object {
                    obj.copy.iter().map(|el| {
                        ManuallyDrop::new(Cc::__new_internal(*el))
                    }).for_each(|el| {
                        el.trace(ctx);
                    });
                }
            }

            // se sei nella prima fase, se il mutex è lockatto ingora, nella seconda fase devi aspettare e continare il tracciamento
            //PS ho fatto un mix, se è accessibile traccio, altrimenti aspetto che sia accessibile e poi traccio
        }
    }

    fn make_copy(&mut self, ctx: &mut CopyContext<'_>) {
        let object = self.log_pointer.mutex.get_mut();
        if let Some(obj) = object.unwrap() {
            ctx.copy_vec.extend(obj.copy.iter());

            return;
        }

        let result = self.mutex.try_lock();

        if let Ok(mut guard) = result {
            guard.make_copy(ctx);
        }
    }
}

impl<T: Trace + ?Sized> Finalize for LoggedMutex<T> {}

