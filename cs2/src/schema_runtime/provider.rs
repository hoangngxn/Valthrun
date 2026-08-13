use std::{
    borrow::Cow,
    collections::BTreeMap,
};

use cs2_schema_provider::{
    OffsetInfo,
    SchemaProvider,
};

use crate::schema_runtime::dump::RuntimeOffset;

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
struct CachedOffset<'a> {
    pub module: Cow<'a, str>,
    pub class: Cow<'a, str>,
    pub member: Cow<'a, str>,
}

pub struct RuntimeSchemaProvider {
    lookup_map: BTreeMap<CachedOffset<'static>, u64>,
}

impl RuntimeSchemaProvider {
    pub fn new(offsets: &[RuntimeOffset]) -> Self {
        let mut lookup_map = BTreeMap::new();
        for offset in offsets {
            lookup_map.insert(
                CachedOffset {
                    module: offset.module.clone().into(),
                    class: offset.class.clone().into(),
                    member: offset.member.clone().into(),
                },
                offset.value,
            );
        }
        Self { lookup_map }
    }
}

impl SchemaProvider for RuntimeSchemaProvider {
    fn resolve_offset(&self, offset: &OffsetInfo) -> Option<u64> {
        let offset = CachedOffset {
            module: offset.module.into(),
            class: offset.class_name.into(),
            member: offset.member.into(),
        };

        self.lookup_map.get(&offset).cloned()
    }
}
