export interface Clipper2Module {
  [key: string]: any;
}

declare function Clipper2Z(moduleArg?: { locateFile?: (path: string) => string }): Promise<Clipper2Module>;

export default Clipper2Z;
