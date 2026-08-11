#!/usr/bin/env node
"use strict";
import defaultValue,*as namespace from "./module.js";
import{source as renamed}from "./named.js";
import data from "./data.json" with{type:"json"};
export{renamed};
export{renamed as value}from "./other.js";
export*from "./all.js";
export*as tools from "./tools.js";
export default function declared(value=defaultValue,...rest){return{value,rest};}
export class Derived extends namespace.Base{static count=0;#value=1;constructor(value){super();this.#value=value;}get value(){return this.#value;}set value(next){this.#value=next;}async method(input){return await input;}*items(){yield this.#value;}static{this.ready=true;}}
const literals=[true,false,null,123,1n,/a+/gi,"text",`hello ${renamed}`];
const array=[,defaultValue,...literals];
const object={renamed,plain:1,[renamed]:2,get current(){return this.plain;},set current(value){this.plain=value;},method(value){return value;},async load(){return import("./lazy.js",{with:{type:"json"}});},*iterate(){yield*array;},...data};
let first,rest;
[first,...rest]=array;
({value:first=0,...rest}=object);
const calculation=first+2*3**2;
const choice=first??(rest.length?rest[0]:defaultValue);
const chained=object?.nested?.method?.(choice)?.value;
const created=new namespace.Widget(first,choice);
const tagged=namespace.tag`value ${created}`;
const expression=(first++,--first,typeof created==="object"&&!false?+first:-first);
async function run(items){for await(const item of items){if(item)continue;else break;}return await Promise.resolve(items);}
function* generate(items){for(const item of items)yield item;}
for(let index=0;index<array.length;index+=1){object[index]=array[index];}
for(const key in object){delete object[key];}
for(const item of array){void item;}
while(first<10)first++;
do first--;while(first>0);
outer:for(;;){switch(first){case 0:first=1;break outer;default:throw new Error("unexpected");}}
try{run(array);}catch(error){console.error(error);}finally{debugger;}
export{array,calculation,chained,choice,created,expression,generate,literals,object,run,tagged};
