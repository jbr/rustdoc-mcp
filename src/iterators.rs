use crate::doc_ref::DocRef;
use fieldwork::Fieldwork;
use rustdoc_types::{Id, Item, ItemEnum, Type};
use std::{collections::hash_map::Values, iter::Chain};

pub(crate) struct MethodIter<'a> {
    item: DocRef<'a, Item>,
    impl_block_iter: InherentImplBlockIter<'a>,
    current_item_iter: Option<std::slice::Iter<'a, Id>>,
}

impl<'a> MethodIter<'a> {
    pub(crate) fn new(item: DocRef<'a, Item>) -> Self {
        let impl_block_iter = InherentImplBlockIter::new(item);
        Self {
            item,
            impl_block_iter,
            current_item_iter: None,
        }
    }
}

impl<'a> DocRef<'a, Item> {
    pub(crate) fn methods(&self) -> MethodIter<'a> {
        MethodIter::new(*self)
    }

    pub(crate) fn traits(&self) -> TraitIter<'a> {
        TraitIter::new(*self)
    }

    pub(crate) fn child_items(&self) -> ChildItems<'a> {
        ChildItems::new(*self)
    }
}

impl<'a, T> DocRef<'a, T> {
    pub(crate) fn id_iter(&self, ids: &'a [Id]) -> IdIter<'a, T> {
        IdIter::new(*self, ids)
    }
}

pub(crate) struct TraitIter<'a> {
    item: DocRef<'a, Item>,
    item_iter: Values<'a, Id, Item>,
}
impl<'a> TraitIter<'a> {
    fn new(item: DocRef<'a, Item>) -> Self {
        let item_iter = item.crate_docs().index.values();
        Self { item, item_iter }
    }
}

impl<'a> Iterator for TraitIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        for item in &mut self.item_iter {
            if let ItemEnum::Impl(impl_block) = &item.inner
                && let Type::ResolvedPath(path) = &impl_block.for_
                && path.id == self.item.id
                && impl_block.trait_.is_some()
            {
                return Some(self.item.build_ref(item));
            }
        }
        None
    }
}

impl<'a> Iterator for MethodIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current_item_iter) = &mut self.current_item_iter {
                for id in current_item_iter {
                    if let Some(item) = self.item.get(id) {
                        return Some(item);
                    }
                }
            }

            if let Some(item) = self.impl_block_iter.next()
                && let ItemEnum::Impl(impl_block) = &item.item().inner
            {
                self.current_item_iter = Some(impl_block.items.iter())
            } else {
                return None;
            }
        }
    }
}

#[derive(Debug, Fieldwork)]
pub(crate) struct IdIter<'a, T> {
    item: DocRef<'a, T>,
    id_iter: std::slice::Iter<'a, Id>,
    glob_iter: Option<Box<IdIter<'a, Item>>>,
}

impl<'a, T> IdIter<'a, T> {
    pub(crate) fn new(item: DocRef<'a, T>, ids: &'a [Id]) -> Self {
        Self {
            item,
            id_iter: ids.iter(),
            glob_iter: None,
        }
    }
}

impl<'a, T> Iterator for IdIter<'a, T> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(glob_iter) = self.glob_iter.as_mut() {
                if let Some(item) = glob_iter.next() {
                    return Some(item);
                } else {
                    self.glob_iter = None;
                }
            }

            for id in &mut self.id_iter {
                if let Some(item) = self.item.get(id) {
                    if let ItemEnum::Use(use_item) = item.inner() {
                        let source_item = use_item
                            .id
                            .and_then(|id| item.crate_docs().get(item.request(), &id))
                            .or_else(|| {
                                item.request().resolve_path(&use_item.source, &mut vec![])
                            })?;

                        if use_item.is_glob {
                            self.glob_iter = match source_item.inner() {
                                ItemEnum::Module(module) => {
                                    Some(Box::new(source_item.id_iter(&module.items)))
                                }
                                ItemEnum::Enum(enum_item) => {
                                    Some(Box::new(source_item.id_iter(&enum_item.variants)))
                                }
                                _ => None,
                            };

                            break;
                        } else {
                            return Some(source_item.with_name(&use_item.name));
                        }
                    }
                    return Some(item);
                }
            }
            if self.glob_iter.is_none() {
                break;
            }
        }

        None
    }
}

pub(crate) struct InherentImplBlockIter<'a> {
    item: DocRef<'a, Item>,
    item_iter: Values<'a, Id, Item>,
}

impl<'a> InherentImplBlockIter<'a> {
    pub(crate) fn new(item: DocRef<'a, Item>) -> Self {
        let item_iter = item.crate_docs().index.values();
        Self { item, item_iter }
    }
}

impl<'a> Iterator for InherentImplBlockIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        for item in &mut self.item_iter {
            if let ItemEnum::Impl(impl_block) = &item.inner
                && let Type::ResolvedPath(path) = &impl_block.for_
                && path.id == self.item.id
                && impl_block.trait_.is_none()
            {
                return Some(DocRef::new(self.item.request(), self.item, item));
            }
        }
        None
    }
}

pub(crate) enum ChildItems<'a> {
    AssociatedMethods(MethodIter<'a>),
    Use,
    Module(IdIter<'a, Item>),
    Enum(Chain<IdIter<'a, Item>, MethodIter<'a>>),
    None,
}

impl<'a> Iterator for ChildItems<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ChildItems::AssociatedMethods(method_iter) => method_iter.next(),
            ChildItems::Use => todo!("child_items for use item"),
            ChildItems::Module(id_iter) => id_iter.next(),
            ChildItems::Enum(id_iter) => id_iter.next(),
            ChildItems::None => None,
        }
    }
}

impl<'a> ChildItems<'a> {
    pub fn new(item: DocRef<'a, Item>) -> Self {
        match &item.item().inner {
            ItemEnum::Module(module) => Self::Module(item.id_iter(&module.items)),
            ItemEnum::Enum(enum_item) => {
                Self::Enum(item.id_iter(&enum_item.variants).chain(item.methods()))
            }
            ItemEnum::Use(_use_item) => ChildItems::Use,
            ItemEnum::Struct(_) => Self::AssociatedMethods(item.methods()),
            _ => Self::None,
        }
    }
}
