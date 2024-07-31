use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::sync::{LockResult, Mutex, MutexGuard, TryLockResult};

use crate::{Cc, Context, Finalize, Trace};
use crate::cc::CcBox;
use crate::trace::ContextInner;

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
    fn log_copy<E>(&self, result: &Result<MutexGuard<'_, T>, E>) {
        if let Ok(r) = result {
            let mut log = self.log_pointer.mutex.lock().unwrap();
            if log.is_none() {
                let mut vec = Vec::new();
                let mut ctx = Context::new(ContextInner::Copy {
                    copy_vec: &mut vec
                });
                r.trace(&mut ctx);
                let obj = ObjectCopy {
                    copy: vec,
                };

                log.replace(obj);
            }
        }
    }


    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        let result = self.mutex.lock();

        self.log_copy(&result);

        result
    }

    pub fn try_lock(&self) -> TryLockResult<MutexGuard<'_, T>> {
        let result = self.mutex.try_lock();

        self.log_copy(&result);

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
}

unsafe impl<T: Trace + ?Sized> Trace for LoggedMutex<T> {
    fn trace(&self, ctx: &mut Context<'_>) {
        match ctx.inner() {
            ContextInner::Copy { copy_vec } => {
                let object = self.log_pointer.mutex.lock().unwrap().take();
                if let Some(obj) = object {
                    copy_vec.extend(obj.copy.iter());

                    return;
                }

                drop(object);


                let result = self.mutex.try_lock();

                if let Ok(guard) = result {
                    guard.trace(ctx);
                }
            }
            _ => {
                let object = self.log_pointer.mutex.lock().unwrap();


                if let Some(obj) = &*object { //FIXME crea un logpoiner solo se si sta collezionando
                    obj.copy.iter().map(|el| {
                        ManuallyDrop::new(Cc::__new_internal(*el))
                    }).for_each(|el| {
                        el.trace(ctx);
                    });


                    return;
                } else {

                    // se sei nella prima fase, se il mutex è lockatto ingora, nella seconda fase devi aspettare e continare il tracciamento
                }
            }
        }
    }
}

impl<T: Trace + ?Sized> Finalize for LoggedMutex<T> {}

