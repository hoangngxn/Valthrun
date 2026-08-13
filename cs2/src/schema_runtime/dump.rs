use std::{
    collections::BTreeMap,
    fs::File,
    io::{
        BufReader,
        BufWriter,
    },
    path::Path,
};

use anyhow::Context;
use serde::{
    Deserialize,
    Serialize,
};
use utils_state::StateRegistry;

use super::generator;
use crate::StateBuildInfo;

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Deserialize, Serialize)]
pub struct RuntimeOffset {
    pub module: String,
    pub class: String,
    pub member: String,
    pub value: u64,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct RuntimeSchemaState {
    pub cs2_revision: String,
    pub cs2_build_datetime: String,

    pub resolved_offsets: BTreeMap<String, u64>,
    pub offsets: Vec<RuntimeOffset>,
}

impl RuntimeSchemaState {
    pub fn load_from_file(file: &Path) -> anyhow::Result<Self> {
        let file = File::open(file).context("open file")?;
        let reader = BufReader::new(file);
        let schema = serde_json::from_reader::<_, Self>(reader).context("parse schema file")?;
        Ok(schema)
    }

    pub fn from_game(registry: &StateRegistry) -> anyhow::Result<Self> {
        let mut schema = Self::default();

        schema.offsets = generator::generate_schema_offsets(registry)?;
        schema.resolved_offsets =
            generator::generate_resolved_offsets(registry).context("module offsets")?;

        {
            let build_info = registry.resolve::<StateBuildInfo>(())?;
            schema.cs2_build_datetime = build_info.build_datetime.clone();
            schema.cs2_revision = build_info.revision.clone();
        }

        Ok(schema)
    }

    pub fn write_to_file(&self, file: &Path) -> anyhow::Result<()> {
        let output = File::options()
            .create(true)
            .truncate(true)
            .write(true)
            .open(file)?;

        let mut output = BufWriter::new(output);
        serde_json::to_writer_pretty(&mut output, self)?;

        Ok(())
    }
}
