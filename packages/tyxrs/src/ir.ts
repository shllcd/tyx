/**
 * The tyx IR as exported by `tyx::ir::Schema::to_json`, field for field.
 */
export interface Schema {
  types: Def[];
}

export type Def = { name: string; module: string; params?: string[] } & Shape;

export type Shape =
  | { kind: "newtype"; ty: Ty }
  | { kind: "struct"; fields: Fields }
  | { kind: "enum"; variants: Variant[]; tag?: string; other?: number };

export interface Fields {
  style: "named" | "tuple";
  fields: Field[];
}

export interface Field {
  name?: string;
  ty: Ty;
  optional?: boolean;
  default?: boolean;
}

export type Variant = { name: string; index?: number } & Payload;

export type Payload =
  | { payload: "unit" }
  | { payload: "newtype"; ty: Ty }
  | { payload: "fields"; fields: Fields };

export type Ty =
  | { kind: "unit" }
  | { kind: "bool" }
  | { kind: "u8" }
  | { kind: "u16" }
  | { kind: "u32" }
  | { kind: "u64" }
  | { kind: "i8" }
  | { kind: "i16" }
  | { kind: "i32" }
  | { kind: "i64" }
  | { kind: "f32" }
  | { kind: "f64" }
  | { kind: "str" }
  | { kind: "bytes" }
  | { kind: "uuid" }
  | { kind: "timestamp" }
  | { kind: "option"; of: Ty }
  | { kind: "seq"; of: Ty }
  | { kind: "set"; of: Ty }
  | { kind: "map"; key: Ty; value: Ty }
  | { kind: "array"; of: Ty; len: number }
  | { kind: "tuple"; of: Ty[] }
  | { kind: "named"; name: string; args?: Ty[] }
  | { kind: "param"; index: number };
