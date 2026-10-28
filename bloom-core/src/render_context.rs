use std::{any::Any, sync::Arc};

use crate::context::ContextMap;

pub(crate) struct RenderContext<N> {
    pub(crate) parent: Arc<N>,
    pub(crate) sibling: Option<Arc<N>>,
    pub(crate) context: ContextMap,
    /// Detached nodes are kept in the tree but not passed to the object model,
    /// e.G. the children of a suspense boundary that is still showing its fallback.
    pub(crate) detached: bool,
}

impl<N> Clone for RenderContext<N> {
    fn clone(&self) -> Self {
        Self {
            parent: self.parent.clone(),
            sibling: self.sibling.clone(),
            context: self.context.clone(),
            detached: self.detached,
        }
    }
}

impl<N> RenderContext<N> {
    pub(crate) fn new(parent: Arc<N>, sibling: Option<Arc<N>>, context: ContextMap) -> Self {
        Self {
            parent,
            sibling,
            context,
            detached: false,
        }
    }

    pub(crate) fn with_parent(&self, parent: Arc<N>) -> Self {
        Self {
            parent,
            sibling: None,
            context: self.context.clone(),
            detached: self.detached,
        }
    }

    pub(crate) fn with_parent_and_sibling(&self, parent: Arc<N>, sibling: Option<Arc<N>>) -> Self {
        Self {
            parent,
            sibling,
            context: self.context.clone(),
            detached: self.detached,
        }
    }

    pub(crate) fn with_sibling(&self, sibling: Option<Arc<N>>) -> Self {
        Self {
            parent: self.parent.clone(),
            sibling,
            context: self.context.clone(),
            detached: self.detached,
        }
    }

    pub(crate) fn with_context(&self, value: Arc<dyn Any + Send + Sync>) -> Self {
        let mut new_context = self.context.as_ref().clone();
        new_context.insert((*value).type_id(), value);
        Self {
            parent: self.parent.clone(),
            sibling: self.sibling.clone(),
            context: Arc::new(new_context),
            detached: self.detached,
        }
    }

    pub(crate) fn detach(&self) -> Self {
        Self {
            detached: true,
            ..self.clone()
        }
    }

    /// The parent to remove nodes from, none if the nodes were never passed to the object model.
    pub(crate) fn attached_parent(&self) -> Option<Arc<N>> {
        if self.detached {
            None
        } else {
            Some(self.parent.clone())
        }
    }

    pub(crate) fn with_sibling_and_context(
        &self,
        sibling: Option<Arc<N>>,
        value: Arc<dyn Any + Send + Sync>,
    ) -> Self {
        let mut new_context = self.context.as_ref().clone();
        new_context.insert((*value).type_id(), value);
        Self {
            parent: self.parent.clone(),
            sibling,
            context: Arc::new(new_context),
            detached: self.detached,
        }
    }
}
