import { ZomeDefinition } from '@holochain-scaffolding/definitions';
import { ScFile, ScNodeType } from '@source-craft/types';
import { mergeStrings, titleCase } from '../utils';
import { snakeCase } from 'lodash-es';

export const libRs = (zomeDefinition: ZomeDefinition): ScFile => ({
  type: ScNodeType.File,
  content: `use holochain_deterministic_integrity::prelude::*;
${mergeStrings(
  zomeDefinition.entry_defs.map(
    entry_def => `
mod ${snakeCase(entry_def.typeDefinition.name)};`,
  ),
)}
${mergeStrings(
  zomeDefinition.entry_defs.map(
    entry_def => `
use ${snakeCase(entry_def.typeDefinition.name)}::${titleCase(entry_def.typeDefinition.name)};`,
  ),
)}

#[hdk_entry_defs]
#[unit_enum(UnitEntryTypes)]
pub enum EntryTypes {
${mergeStrings(
  zomeDefinition.entry_defs.map(
    entry_def => `#[entry_def(required_validations = 5)]
${titleCase(entry_def.typeDefinition.name)}(${titleCase(entry_def.typeDefinition.name)}),
`,
  )
)}
}

#[hdk_extern]
pub fn validate(_op: Op) -> ExternResult<ValidateCallbackResult> {
  Ok(ValidateCallbackResult::Valid)
}
`,
});
