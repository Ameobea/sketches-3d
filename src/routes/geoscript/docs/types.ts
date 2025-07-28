import type { Composition, CompositionVersion } from 'src/geoscript/geotoyAPIClient';

export interface BuiltinFnSignature {
  arg_defs: ArgDef[];
  description: string;
  return_type: string[];
}

export interface ArgDef {
  name: string;
  valid_types: string[];
  default_value: 'Required' | { Optional: [string] };
  description: string;
}

export interface FnExample {
  composition_id: number;
}

export interface PopulatedFnExample extends FnExample {
  composition: Composition;
  version: CompositionVersion;
}

export interface BuiltinFnDef {
  module: string;
  signatures: BuiltinFnSignature[];
  examples: PopulatedFnExample[];
  aliases: string[];
}

export interface UnpopulatedBuiltinFnDef {
  module: string;
  signatures: BuiltinFnSignature[];
  examples: FnExample[];
  aliases: string[];
}

export type UnpopulatedBuiltinFnDefs = Record<string, UnpopulatedBuiltinFnDef>;

export type BuiltinFnDefs = Record<string, BuiltinFnDef>;
