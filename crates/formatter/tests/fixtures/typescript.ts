interface User<T extends string=string>{readonly id:number;name:T;}type Maybe<T>=T|null|undefined;const user:User<string>={id:1,name:"x"} satisfies User<string>;
