use std::{
    collections::BTreeMap,
    ops::Deref,
};

use anyhow::Context;
use cs2_schema_cutl::{
    CStringUtil,
    FixedCStringUtil,
};
use raw_struct::Reference;
use utils_state::StateRegistry;

use crate::{
    schema::{
        CSchemaSystem,
        CSchemaTypeDeclaredClass,
    },
    schema_runtime::dump::RuntimeOffset,
    CS2Offset,
    Module,
    StateCS2Handle,
    StateCS2Memory,
    StateResolvedOffset,
};

pub fn generate_schema_offsets(states: &StateRegistry) -> anyhow::Result<Vec<RuntimeOffset>> {
    let cs2 = states.resolve::<StateCS2Handle>(())?;
    let memory = states.resolve::<StateCS2Memory>(())?;

    let schema_system = states.resolve::<StateResolvedOffset>(CS2Offset::SchemaSystem)?;
    let system_instance =
        Reference::<dyn CSchemaSystem>::new(memory.view_arc(), schema_system.address);

    let scopes = system_instance.scopes()?;
    let scope_size = scopes.size()? as usize;
    log::debug!(
        "Schema system located at 0x{:X} (0x{:X}) containing 0x{:X} scopes",
        schema_system.address,
        cs2.module_address(Module::Schemasystem, schema_system.address)
            .context("invalid schema system address")?,
        scope_size
    );

    if scope_size > 0x20 {
        anyhow::bail!("Too many scopes ({}). Something went wrong?", scope_size);
    }

    let mut offsets = Vec::new();
    offsets.reserve(0x1000);

    for scope_ptr in scopes
        .data()?
        .elements(memory.view(), 0..scopes.size()? as usize)?
    {
        let scope = scope_ptr
            .value_copy(memory.view())?
            .context("scope nullptr")?;

        let scope_name = scope.scope_name()?.to_string_lossy().to_string();
        log::trace!("Name: {} @ {:X}", scope_name, scope_ptr.address);

        let declared_classes = scope.type_declared_class()?;
        let declared_classes = declared_classes
            .elements()?
            .elements_copy(memory.view(), 0..declared_classes.entry_count()? as usize)?;

        for rb_node in declared_classes {
            let declared_class = rb_node
                .value()?
                .value
                .cast::<dyn CSchemaTypeDeclaredClass>()
                .value_reference(memory.view_arc())
                .context("tree null entry")?;

            let Some(binding) = declared_class.declaration()?.value_copy(memory.view())? else {
                /* steamaudio.dll seems to register a nullptr .... */
                continue;
            };

            let (class_type_scope_name, class_name) =
                crate::read_class_scope_and_name(states, binding.deref())?;
            log::trace!(
                "   {:X} {} -> {}",
                declared_class.declaration()?.address,
                class_name,
                class_type_scope_name
            );
            if !["client.dll", "!GlobalTypes"].contains(&class_type_scope_name.as_str()) {
                continue;
            }

            for class_member in binding
                .fields()?
                .elements(memory.view(), 0..binding.field_size()? as usize)?
            {
                let member_name = class_member
                    .name()?
                    .read_string(memory.view())?
                    .context("missing class member name")?;
                let member_offset = class_member.offset()? as u64;

                offsets.push(RuntimeOffset {
                    module: class_type_scope_name.clone(),
                    class: class_name.clone().to_string(),
                    member: member_name.to_string(),
                    value: member_offset,
                });
            }
        }
    }

    Ok(offsets)
}

pub fn generate_resolved_offsets(states: &StateRegistry) -> anyhow::Result<BTreeMap<String, u64>> {
    let mut result = BTreeMap::<String, u64>::new();

    for offset in CS2Offset::available_offsets() {
        let resolved = states.resolve::<StateResolvedOffset>(*offset)?;
        result.insert(offset.cache_name().to_string(), resolved.offset);
    }

    Ok(result)
}
