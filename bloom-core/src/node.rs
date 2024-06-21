use std::sync::Arc;

pub trait Node {
    fn paint(&self, parent: &Arc<Self>, sibling: &Option<Arc<Self>>);
    fn update(&self, next: &Arc<Self>);
    fn remove(&self, parent: &Arc<Self>);
}
