use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Weak},
};

pub(crate) type ContextMap = HashMap<TypeId, Arc<dyn Any + Send + Sync + 'static>>;

#[derive(Default)]
pub(crate) struct PartialRenderingContext {
    context: Arc<ContextMap>,
    subscribers: Vec<Weak<dyn Fn(ContextMap)>>,
}

thread_local! {
    static CONTEXT: RefCell<HashMap<u64, PartialRenderingContext>> = RefCell::new(HashMap::new());
}

struct ContextSubscription {
    sub: Arc<dyn Fn(ContextMap)>,
}

pub(crate) fn subscribe_to_context<H>(
    id: u64,
    handler: H,
) -> (ContextSubscription, Arc<ContextMap>) {
    CONTEXT.with_borrow_mut(|all_context| {
        let entry = all_context
            .entry(id)
            .or_insert_with(PartialRenderingContext::default);

        let handler = Arc::new(handler);

        entry.subscribers.push(Arc::downgrade(handler.clone()));

        (handler, entry.context.clone())
    })
}

pub(crate) fn set_context(id: u64, ctx: Arc<ContextMap>) {
    CONTEXT.with_borrow_mut(|all_context| {
        let entry = all_context
            .entry(id)
            .or_insert_with(PartialRenderingContext::default);

        for subscriber in entry.subscribers {
            subscriber(ctx.clone())
        }
        entry.context = ctx;
    })
}
