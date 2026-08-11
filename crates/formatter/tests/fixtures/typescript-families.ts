declare const uniqueValue:unique symbol;
type Primitives=any|bigint|boolean|never|null|number|object|string|symbol|undefined|unknown|void|this;
type LiteralTypes="text"|42|42n|true|false;
type Arrays=readonly string[];
type Tuple=[first:string,second?:number,...rest:boolean[]];
type FunctionType=<T>(value:T)=>T;
type Constructor=abstract new<T>(value:T)=>{value:T};
type Conditional<T>=T extends infer U?U:never;
type Intersection={left:string}&{right:number};
type Indexed<T extends{items:unknown[]}>=T["items"][number];
type Mapped<T>={readonly[K in keyof T as `get${Capitalize<string&K>}`]-?:()=>T[K]};
type Imported=import("./module.js",{with:{"resolution-mode":"import"}}).Value<string>;
type Query=typeof import("./module.js");
type Predicate=(value:unknown)=>value is string;
type Assertion=(value:unknown)=>asserts value is string;
type Parenthesized=(string|number)[];
type Template<T extends string>=`prefix-${T}`;
interface Base<T>{readonly value:T;optional?:string;method<U>(value:U):T;new(value:T):Base<T>;[key:string]:unknown;}
interface Extended<T=string>extends Base<T>{kind:"extended";}
enum Kind{Zero,One=1,Text="text",Computed=1+2}
namespace Library{export type Result<T>=Promise<T>;export const value=1;}
declare global{interface Window{worsier:boolean;}}
declare module "virtual"{export const value:string;}
import type{Base as ImportedBase}from "./base.js";
import Alias=require("./legacy.cjs");
export import ExportedAlias=Alias;
export as namespace Worsier;
export=Library;
declare function overloaded(value:string):number;
declare function overloaded(value:number):string;
abstract class Service<T>{abstract readonly value:T;private secret?:string;protected accessor current:T;static{this.ready=true;}constructor(public input:T){}abstract method<U>(value:U):T;}
const satisfies={value:"text",method:<U>(value:U)=>"text"}satisfies Partial<Base<string>>;
const asserted=<Base<string>>satisfies;
const cast=asserted as unknown as Base<string>;
const nonNull=cast!;
const instantiated=overloaded<string>;
function identity<const T>(value:T):T{return value;}
const generic=<T,>(value:T):T=>value;
