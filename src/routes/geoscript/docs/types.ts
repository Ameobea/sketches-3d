export interface BuiltinFnDef {
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

export type BuiltinFnDefs = Record<string, BuiltinFnDef[]>;
