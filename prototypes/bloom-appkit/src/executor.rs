//! A single-future executor that polls on the main GCD queue, which
//! NSApplication's run loop drains. The render loop owns AppKit views and must
//! run here; component render futures are Send and go to a thread pool.
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
};

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;

type RootFuture = Pin<Box<dyn Future<Output = ()>>>;

thread_local! {
    static ROOT: RefCell<Option<RootFuture>> = const { RefCell::new(None) };
}

struct MainWaker {
    scheduled: AtomicBool,
}

impl Wake for MainWaker {
    fn wake(self: Arc<Self>) {
        // coalesce: many state updates between two polls cost one poll
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            DispatchQueue::main().exec_async(move || poll_root(&self));
        }
    }
}

fn poll_root(waker: &Arc<MainWaker>) {
    waker.scheduled.store(false, Ordering::Release);
    ROOT.with(|root| {
        let mut slot = root.borrow_mut();
        if let Some(future) = slot.as_mut() {
            let w = Waker::from(waker.clone());
            if future
                .as_mut()
                .poll(&mut Context::from_waker(&w))
                .is_ready()
            {
                *slot = None;
            }
        }
    });
}

/// Run `future` on the main thread, interleaved with AppKit event handling.
pub fn spawn_main(_mtm: MainThreadMarker, future: impl Future<Output = ()> + 'static) {
    ROOT.with(|root| *root.borrow_mut() = Some(Box::pin(future)));
    Arc::new(MainWaker {
        scheduled: AtomicBool::new(false),
    })
    .wake();
}
